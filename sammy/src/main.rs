extern crate config;
#[macro_use]
extern crate log;
extern crate serde;
#[macro_use]
extern crate serde_derive;

use hyper;
use hyper::Client;
use hyper_tls;
use hyper_tls::HttpsConnector;
use log::{info, trace, warn};
use samsara::apis::configuration::Configuration;
use samsara::apis::VehicleStatsApi;
use serde_json;
use simplelog::{ColorChoice, CombinedLogger, LevelFilter, TermLogger, WriteLogger, TerminalMode};
use std::fs::File;

use crate::settings::{Database, Settings};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use tokio_postgres::{Error, NoTls, Config};
use std::time::Duration;

// TODO: Move settings library to where the rest of the tools can see it.
mod settings;

// FIXME: Items to insert into the database. Reduces the chance of losing items when the database is down.
const DB_INSERT_QUEUE_MAX_SIZE: usize = 1000000;
const DB_CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

const VEHICLE_STAT_NAMES: &[&str] = &["gps", "engineStates", "obdOdometerMeters"];

const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
const PKG_NAME: &str = env!("CARGO_PKG_NAME");

/// Convert the database configuration in the settings to a tokio-postgres config.
impl Into<Config> for Database {
    fn into(self) -> Config {
        let mut config = Config::new();
        if let Some(user) = self.user {
            config.user(&user);
        }
        if let Some(password) = self.password {
            config.password(&password);
        }
        config.host(&self.host)
            .port(self.port.unwrap_or(5432))
            .dbname(&self.name)
            .application_name(PKG_NAME)
            .connect_timeout(DB_CONNECT_TIMEOUT)
            .clone()
        // TODO SSL is useful if not connecting to localhost: .ssl_mode(..)
    }
}

#[derive(Debug)]
struct VehicleStat {
    time: DateTime<Utc>,
    samsara_id: Option<String>,
    code: String,
    kind: String,
    json: String,
}

/// The responses from the Samsara API are logged to the console. Selection portions of the data
/// are written to the Transporter database.
#[tokio::main]
pub async fn main() -> std::io::Result<()> {
    let APP_NAME = format!("{} {}", PKG_NAME, PKG_VERSION);

    // Logging
    let log_level = LevelFilter::Info;
    let log_config = simplelog::ConfigBuilder::new()
        .set_time_format("%F %T".to_string())
        .build();
    let log_path = format!("{}.log", PKG_NAME);
    CombinedLogger::init(vec![
        TermLogger::new(
            log_level,
            log_config.clone(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        // FIXME: Does this overwrite the old log?
        WriteLogger::new(
            log_level,
            log_config.clone(),
            File::create(log_path).unwrap(),
        ),
    ])
        .unwrap();
    log::info!("{} started", APP_NAME);

    let settings = Settings::new().unwrap();

    let https_connector = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https_connector);

    let samsara_config = Configuration {
        oauth_access_token: Some(settings.samsara.api_token),
        user_agent: Some(APP_NAME),
        ..Configuration::new(client)
    };

    let mut db_insert_queue = VecDeque::new();

    let vehicle_stats_api =
        samsara::apis::VehicleStatsApiClient::new(std::rc::Rc::new(samsara_config));
    let vehicle_stats_future = vehicle_stats_api.get_vehicle_stats(
        VEHICLE_STAT_NAMES
            .iter()
            .map(|s| String::from(*s))
            .collect(),
        None,
        None,
        None,
        None,
        None,
    );
    let vehicle_stats: Vec<VehicleStat> = match vehicle_stats_future.await {
        Ok(response) => response
            .data
            .iter()
            .map(|resp| {
                let json_value = serde_json::to_value(&resp).expect("serialize result");
                let json_obj = json_value.as_object().expect("JSON object");
                let json_str = serde_json::to_string(&json_obj).unwrap();
                info!("vehicle stats response: {}", json_str);

                match json_obj["name"].as_str() {
                    Some(code) => {
                        json_obj
                            .iter()
                            .filter(|(_, value)| value.is_object())
                            .map(|(kind, value)| {
                                let value_obj = value.as_object().unwrap();
                                let time = match value_obj["time"].as_str().map(|s| match s
                                    .parse::<DateTime<Utc>>()
                                {
                                    Ok(time) => time,
                                    Err(e) => {
                                        error!(
                                            "Cannot parse time, using current time: {:?}",
                                            json_obj["time]"]
                                        );
                                        Utc::now()
                                    }
                                }) {
                                    Some(time) => time,
                                    None => {
                                        warn!("No time provided, using current time");
                                        Utc::now()
                                    }
                                };

                                // Remove the time from the value to save space.
                                let mut pruned_values: HashMap<String, Value> = HashMap::new();
                                value_obj.iter().for_each(|(key, value)| {
                                    if key != "time" {
                                        pruned_values.insert(key.clone(), value.clone());
                                    }
                                });

                                Some(VehicleStat {
                                    time,
                                    samsara_id: json_obj["id"].as_str().map(|s| s.to_string()),
                                    code: code.to_string(),
                                    kind: kind.clone(),
                                    json: serde_json::to_string(&pruned_values).unwrap(),
                                })
                            })
                            .flatten()
                            .collect()
                    }
                    _ => {
                        error!("Missing code in vehicle stat response");
                        Vec::new()
                    }
                }
            })
            .flatten()
            .collect(),
        Err(e) => {
            error!("Cannot retrieve vehicle stats: {:?}", e);
            Vec::new()
        }
    };

    // Add to the DB insert queue but cap at a maximum size.
    vehicle_stats
        .iter()
        .for_each(|stat| db_insert_queue.push_front(stat));
    db_insert_queue.truncate(DB_INSERT_QUEUE_MAX_SIZE);

    let postgres_config: Config = settings.transporter.database.into();
    match postgres_config.connect(NoTls).await {
        Ok((client, connection)) => {
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    eprintln!("connection error: {}", e);
                }
            });
            let rows = client
                .query("SELECT $1::TEXT", &[&"hello world"])
                .await.unwrap(); // FIXME: Unwrap;
            let value: &str = rows[0].get(0);
            assert_eq!(value, "hello world");
        }
        Err(e) => error!("Cannot connect to the database: {:?}", e)
    };

    // {
    //     "engineState": {
    //     "time": "2021-07-13T20:10:23Z",w
    //     "value": "Off"
    // },
    //     "gps": {
    //     "address": {
    //         "id": "8150007",
    //         "name": "Joe Martin & Sons Yard"
    //     },
    //     "headingDegrees": 0,
    //     "isEcuSpeed": false,
    //     "latitude": 53.70296222,
    //     "longitude": -113.18734387,
    //     "reverseGeo": {
    //         "formattedLocation": "83 Street, AB"
    //     },
    //     "speedMilesPerHour": 0,
    //     "time": "2021-07-14T02:13:53Z"
    //     },
    //     "id": "212014919004270",
    //     "name": "717",
    //     "obdOdometerMeters": {
    //     "time": "2021-07-13T20:10:22Z",
    //     "value": 386814875
    // }
    // }
    Ok(())
}
