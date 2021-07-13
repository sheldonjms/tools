use config::{Config, ConfigError};

#[derive(Debug, Deserialize)]
struct Database {
    url: String,
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

#[derive(Debug, Deserialize)]
pub struct Settings {
    //    database: Database,
    pub http: Http,
    pub samsara: Samsara,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let mut settings = Config::default();
        settings.merge(config::File::with_name("settings")).unwrap();
        settings.merge(config::Environment::with_prefix("SETTINGS")).unwrap();
        settings.try_into()
    }
}
