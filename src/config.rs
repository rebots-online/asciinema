use anyhow::{anyhow, bail, Result};
use reqwest::Url;
use serde::Deserialize;
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const DEFAULT_SERVER_URL: &str = "https://asciinema.org";
const INSTALL_ID_FILENAME: &str = "install-id";

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Config {
    server: Server,
    cmd: Cmd,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Server {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Cmd {
    rec: Rec,
    play: Play,
}

#[derive(Debug, Deserialize, Default)]
#[allow(unused)]
pub struct Rec {
    pub input: bool,
    pub command: Option<String>,
    pub env: String,
    pub idle_time_limit: Option<f64>,
    pub prefix_key: Option<String>,
    pub pause_key: String,
    pub add_marker_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(unused)]
pub struct Play {
    pub speed: f64,
    pub idle_time_limit: Option<f64>,
    pub pause_key: Option<String>,
    pub step_key: Option<String>,
    pub next_marker_key: Option<String>,
}

impl Config {
    pub fn new(server_url: Option<String>) -> Result<Self> {
        let mut config = config::Config::builder()
            .set_default("server.url", None::<Option<String>>)?
            .set_default("cmd.rec.input", false)?
            .set_default("cmd.rec.env", "SHELL,TERM")?
            .set_default("cmd.rec.pause_key", "C-\\")?
            .set_default("cmd.play.speed", 1.0)?
            .add_source(
                config::File::with_name(&user_defaults_path()?.to_string_lossy()).required(false),
            )
            .add_source(
                config::File::with_name(&user_config_path()?.to_string_lossy()).required(false),
            )
            .add_source(config::Environment::with_prefix("asciinema").separator("_"));

        if let Some(url) = server_url {
            config = config.set_override("server.url", Some(url))?;
        }

        if let (Err(_), Ok(url)) = (
            env::var("ASCIINEMA_SERVER_URL"),
            env::var("ASCIINEMA_API_URL"),
        ) {
            env::set_var("ASCIINEMA_SERVER_URL", url);
        }

        Ok(config.build()?.try_deserialize()?)
    }

    pub fn get_server_url(&self) -> Result<Url> {
        match self.server.url.as_ref() {
            Some(url) => Ok(Url::parse(url)?),

            None => {
                let url = Url::parse(&ask_for_server_url()?)?;
                save_default_server_url(url.as_ref())?;

                Ok(url)
            }
        }
    }

    pub fn get_install_id(&self) -> Result<String> {
        let path = install_id_path()?;

        if let Some(id) = read_install_id(&path)? {
            Ok(id)
        } else {
            let id = create_install_id();
            save_install_id(&path, &id)?;

            Ok(id)
        }
    }

    pub fn cmd_play_pause_key(&self) -> Result<Option<Option<char>>> {
        self.cmd.play.pause_key.as_ref().map(parse_key).transpose()
    }

    pub fn cmd_play_step_key(&self) -> Result<Option<Option<char>>> {
        self.cmd.play.step_key.as_ref().map(parse_key).transpose()
    }

    pub fn cmd_play_next_marker_key(&self) -> Result<Option<Option<char>>> {
        self.cmd
            .play
            .next_marker_key
            .as_ref()
            .map(parse_key)
            .transpose()
    }
}

fn ask_for_server_url() -> Result<String> {
    println!("No asciinema server configured for this CLI.");
    let mut rl = rustyline::DefaultEditor::new()?;
    let url = rl.readline_with_initial(
        "Enter the server URL to use by default: ",
        (DEFAULT_SERVER_URL, ""),
    )?;
    println!();

    Ok(url)
}

fn save_default_server_url(url: &str) -> Result<()> {
    let path = user_defaults_path()?;

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }

    fs::write(path, format!("[server]\nurl = \"{url}\"\n"))?;

    Ok(())
}

fn read_install_id(path: &PathBuf) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s.trim().to_string())),

        Err(e) => {
            if e.kind() == ErrorKind::NotFound {
                Ok(None)
            } else {
                bail!(e)
            }
        }
    }
}

fn create_install_id() -> String {
    Uuid::new_v4().to_string()
}

fn save_install_id(path: &PathBuf, id: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }

    fs::write(path, id)?;

    Ok(())
}

fn user_config_path() -> Result<PathBuf> {
    Ok(home()?.join("config.toml"))
}

fn user_defaults_path() -> Result<PathBuf> {
    Ok(home()?.join("defaults.toml"))
}

fn install_id_path() -> Result<PathBuf> {
    Ok(home()?.join(INSTALL_ID_FILENAME))
}

fn home() -> Result<PathBuf> {
    env::var("ASCIINEMA_CONFIG_HOME")
        .map(PathBuf::from)
        .or(env::var("XDG_CONFIG_HOME").map(|home| Path::new(&home).join("asciinema")))
        .or(env::var("HOME").map(|home| Path::new(&home).join(".config").join("asciinema")))
        .map_err(|_| anyhow!("need $HOME or $XDG_CONFIG_HOME or $ASCIINEMA_CONFIG_HOME"))
}

fn parse_key<S: AsRef<str>>(key: S) -> Result<Option<char>> {
    let key = key.as_ref();
    let chars: Vec<char> = key.chars().collect();

    match chars.len() {
        0 => return Ok(None),
        1 => return Ok(Some(chars[0])),

        2 => {
            if chars[0] == '^' {
                let key = (chars[1].to_ascii_uppercase() as u8 - 0x40) as char;

                return Ok(Some(key));
            }
        }

        3 => {
            if chars[0].to_ascii_uppercase() == 'C' && ['+', '-'].contains(&chars[1]) {
                let key = (chars[2].to_ascii_uppercase() as u8 - 0x40) as char;

                return Ok(Some(key));
            }
        }

        _ => (),
    }

    Err(anyhow!("invalid key definition '{key}'"))
}