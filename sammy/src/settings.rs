use config::{Config, ConfigError};
use std::path::Path;

const ENV_VAR_PREFIX: &str = "SAMMY";

#[derive(Debug, Deserialize)]
pub struct Database {
    pub host: String,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct Http {
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct Samsara {
    /// Samsara calls it an API token but the OpenAPI Generator calls it an API key.
    #[doc(alias = "api_key")]
    pub api_token: String,
}

/// Settings to talk to the Transporter application
#[derive(Debug, Deserialize)]
pub struct Transporter {
    pub database: Database,
}

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub http: Http,
    pub samsara: Samsara,
    pub transporter: Transporter,
}

impl Settings {
    pub fn new<P: AsRef<Path>>(config_file: P) -> Result<Self, ConfigError> {
        let config = Config::builder()
            .add_source(config::File::from(config_file.as_ref()))
            .add_source(config::Environment::with_prefix(ENV_VAR_PREFIX))
            .build()?;
        let settings: Settings = config.try_deserialize()?;
        Ok(settings)
    }
}
