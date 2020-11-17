use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use structopt::{clap, StructOpt};
use warp::http::Uri;

use std::fs::File;
use std::io::Read;
use std::net::IpAddr;
use std::path::PathBuf;

type Bytes = Vec<u8>;

#[derive(Debug, Default, StructOpt, Serialize, Deserialize)]
pub struct ConfigBuilder {
    #[serde(deserialize_with = "deserialize_base64")]
    #[serde(serialize_with = "serialize_base64")]
    #[serde(default)]
    #[structopt(long = "session_key", parse(try_from_str = parse_base64))]
    /// The secret key to use for storing session data
    pub session_key: Option<Bytes>,
    #[serde(deserialize_with = "deserialize_uri")]
    #[serde(serialize_with = "serialize_uri")]
    #[serde(default)]
    #[structopt(short = "u", long = "url")]
    /// The base url of the website
    pub base_url: Option<Uri>,
    #[serde(deserialize_with = "deserialize_uri")]
    #[serde(serialize_with = "serialize_uri")]
    #[serde(default)]
    #[structopt(long = "oauth_login")]
    /// The end point to send oauth redirect to
    pub oauth_login_url: Option<Uri>,
    #[serde(deserialize_with = "deserialize_uri")]
    #[serde(serialize_with = "serialize_uri")]
    #[serde(default)]
    #[structopt(long = "oauth_token")]
    /// The end point to get oauth tokens from
    pub oauth_token_url: Option<Uri>,
    #[serde(default)]
    #[structopt(long = "oauth_id")]
    /// The oauth client id
    pub oauth_id: Option<String>,
    #[serde(default)]
    #[structopt(long = "oauth_secret")]
    /// The oauth client secret
    pub oauth_secret: Option<String>,
    #[serde(default)]
    #[structopt(short = "i", long = "ip")]
    /// The ip address to listen on [default: 0.0.0.0]
    pub listen_ip: Option<IpAddr>,
    #[serde(default)]
    #[structopt(short = "p", long = "port")]
    /// The port to listen on [default: 80]
    pub listen_port: Option<u16>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_uri")]
    #[serde(serialize_with = "serialize_uri")]
    #[structopt(short = "r", long = "redis")]
    /// The connection string to the redis datastore
    pub redis_url: Option<Uri>,
    #[serde(skip)]
    #[structopt(short = "c", long = "config", default_value = "./config.toml")]
    /// The config file to load default settings from
    pub config_file: PathBuf,
    #[serde(skip)]
    #[structopt(short = "v", parse(from_occurrences))]
    /// The verbosity level of logging
    pub verbosity: u8,
    #[serde(skip)]
    #[structopt(short = "s", long = "silent")]
    /// Disable all logging
    pub silent: bool,
    #[serde(skip)]
    #[structopt(subcommand)]
    pub cmd: Option<Subcommand>,
}

#[derive(Debug, StructOpt, Serialize, Deserialize)]
pub enum Subcommand {
    #[structopt(name = "config")]
    /// Generate an example config.toml file
    GenerateConfig,
}

impl ConfigBuilder {
    fn build(self) -> Result<Config, &'static str> {
        let config = Config {
            session_key: self.session_key.ok_or_else(|| "session_key")?,
            base_url: self.base_url.ok_or_else(|| "base_url")?,
            oauth_login_url: self.oauth_login_url.ok_or_else(|| "oauth_login_url")?,
            oauth_token_url: self.oauth_token_url.ok_or_else(|| "oauth_token_url")?,
            oauth_id: self.oauth_id.ok_or_else(|| "oauth_id")?,
            oauth_secret: self.oauth_secret.ok_or_else(|| "oauth_secret")?,
            listen_ip: self.listen_ip.unwrap_or([0, 0, 0, 0].into()),
            listen_port: self.listen_port.unwrap_or(80),
            redis_url: self.redis_url.ok_or_else(|| "redis_url")?,
            verbosity: self.verbosity,
            silent: self.silent,
        };

        Ok(config)
    }

    fn merge(self, other: ConfigBuilder) -> ConfigBuilder {
        ConfigBuilder {
            session_key: self.session_key.or(other.session_key),
            base_url: self.base_url.or(other.base_url),
            oauth_login_url: self.oauth_login_url.or(other.oauth_login_url),
            oauth_token_url: self.oauth_token_url.or(other.oauth_token_url),
            oauth_id: self.oauth_id.or(other.oauth_id),
            oauth_secret: self.oauth_secret.or(other.oauth_secret),
            listen_ip: self.listen_ip.or(other.listen_ip),
            listen_port: self.listen_port.or(other.listen_port),
            redis_url: self.redis_url.or(other.redis_url),
            config_file: self.config_file,
            verbosity: self.verbosity,
            silent: self.silent,
            cmd: self.cmd.or(other.cmd),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub session_key: Vec<u8>,
    pub oauth_login_url: Uri,
    pub oauth_token_url: Uri,
    pub oauth_id: String,
    pub oauth_secret: String,
    pub listen_ip: IpAddr,
    pub listen_port: u16,
    pub redis_url: Uri,
    pub base_url: Uri,
    pub silent: bool,
    pub verbosity: u8,
}

impl Config {
    pub fn load() -> Config {
        ConfigBuilder::from_args();
        let settings = ConfigBuilder::from_args();

        match settings.cmd {
            Some(Subcommand::GenerateConfig) => {
                let default = ConfigBuilder {
                    session_key: vec![0, 1, 2, 3, 4, 5].into(),
                    base_url: Uri::from_static("http://example.com").into(),
                    oauth_login_url: Uri::from_static("http://example.com").into(),
                    oauth_token_url: Uri::from_static("http://example.com").into(),
                    oauth_id: Some("oauth_id".into()),
                    oauth_secret: Some("oauth_secret".into()),
                    listen_ip: Some([0, 0, 0, 0].into()),
                    listen_port: 80.into(),
                    redis_url: Uri::from_static("redis://server:port/db").into(),
                    ..Default::default()
                };

                let settings = toml::to_string_pretty(&default).unwrap_or_else(|e| {
                    config_err(
                        format!("Unable to generate sample config: {:?}", e),
                        clap::ErrorKind::Io,
                    )
                });

                println!("{}", settings);
                std::process::exit(0)
            }
            _ => (),
        }

        let mut config_file = String::new();
        let mut f = File::open(&settings.config_file).unwrap_or_else(|e| {
            config_err(
                format!("Unable to open config file: {:?}", e),
                clap::ErrorKind::Io,
            )
        });
        f.read_to_string(&mut config_file).unwrap_or_else(|e| {
            config_err(
                format!("Unable to read config file: {:?}", e),
                clap::ErrorKind::Io,
            )
        });
        let config_file_settings: ConfigBuilder =
            toml::from_str(&config_file).unwrap_or_else(|e| {
                config_err(
                    format!("Unable to load config file: {:?}", e),
                    clap::ErrorKind::Io,
                )
            });
        let settings = settings.merge(config_file_settings);

        settings.build().unwrap_or_else(|e| {
            config_err(
                format!("Missing required value: {}", e),
                clap::ErrorKind::MissingRequiredArgument,
            )
        })
    }
}

fn config_err(msg: impl AsRef<str>, error: structopt::clap::ErrorKind) -> ! {
    let error = clap::Error::with_description(msg.as_ref(), error);
    error.exit()
}

fn parse_base64(src: &str) -> Result<Vec<u8>, base64::DecodeError> {
    base64::decode(src)
}

fn serialize_base64<S: Serializer>(
    bytes: &Option<Vec<u8>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    bytes
        .as_ref()
        .map(|bytes| base64::encode(bytes.as_slice()))
        .serialize(serializer)
}

fn deserialize_base64<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<u8>>, D::Error> {
    let s = Option::<String>::deserialize(deserializer)?;
    match s {
        Some(s) => base64::decode(&s).map_err(de::Error::custom).map(Some),
        None => Ok(None),
    }
}

fn serialize_uri<S: Serializer>(url: &Option<Uri>, serializer: S) -> Result<S::Ok, S::Error> {
    let s = url.as_ref().map(|u| u.to_string());
    s.serialize(serializer)
}

fn deserialize_uri<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Uri>, D::Error> {
    let s = Option::<String>::deserialize(deserializer)?;
    match s {
        Some(s) => s.parse::<Uri>().map_err(de::Error::custom).map(Some),
        None => Ok(None),
    }
}
