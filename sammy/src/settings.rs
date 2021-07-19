use config::{Config, ConfigError};

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
    pub fn new() -> Result<Self, ConfigError> {
        let mut settings = Config::default();
        settings.merge(config::File::with_name("settings")).unwrap();
        settings
            .merge(config::Environment::with_prefix("SETTINGS"))
            .unwrap();
        settings.try_into()
    }
}
