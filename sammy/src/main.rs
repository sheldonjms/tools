extern crate config;
#[macro_use]
extern crate log;
extern crate serde;
#[macro_use]
extern crate serde_derive;

use std::collections::{HashMap, VecDeque};
use std::ffi::OsStr;
use std::time::Duration;

use chrono::{DateTime, NaiveDateTime, Utc};
use clap::{Arg, Command, crate_authors, crate_description, crate_name, crate_version, ValueHint};
use hyper::Client;
use hyper_tls::HttpsConnector;
use log::{info, trace, warn};
use serde_json::Value;
use simplelog::{ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, TerminalMode, TermLogger, WriteLogger};
use tokio_postgres::{Config, NoTls};

use samsara::apis::configuration::Configuration;
use samsara::apis::VehicleStatsApi;

use crate::settings::{Database, Settings};

// TODO: Move settings library to where the rest of the tools can see it.
mod settings;

// FIXME: Items to insert into the database. Reduces the chance of losing items when the database is down.
const DB_INSERT_QUEUE_MAX_SIZE: usize = 1_000_000;
const DB_CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

const VEHICLE_STAT_NAMES: &[&str] = &["gps", "engineStates", "obdOdometerMeters"];

/// Convert the database configuration in the settings to a tokio-postgres config.
impl From<settings::Database> for Config {
    fn from(db: Database) -> Self {
        let mut config = Config::new();
        if let Some(user) = db.user {
            config.user(&user);
        }
        if let Some(password) = db.password {
            config.password(&password);
        }
        config.host(&db.host)
            .port(db.port.unwrap_or(5432))
            .dbname(&db.name)
            .application_name(crate_name!())
            .connect_timeout(DB_CONNECT_TIMEOUT)
            .clone()
        // TODO SSL is useful if not connecting to localhost: .ssl_mode(..)
    }
}

#[derive(Debug)]
struct VehicleStat {
    /// DateTime<Utc> is not used because Slick 3.3 cannot parse timestamp with
    /// timezone in a PostgreSQL database.
    time: NaiveDateTime,
    samsara_id: Option<String>,
    code: String,
    kind: String,
    json: String,
}

/// The responses from the Samsara API are logged to the console. Selection portions of the data
/// are written to the Transporter database.
#[tokio::main]
pub async fn main() -> std::io::Result<()> {

    // Command line arguments
    let cli_matches = Command::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!())
        .about(crate_description!())
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .multiple_occurrences(true),
        )
                .arg(
                    Arg::new("CONFIG_FILE")
                        .help("Name of the configuration file")
                        .value_hint(ValueHint::AnyPath)
                        .required(true),
                )
        .get_matches();

    let log_level_filter = [
        LevelFilter::Off,
        LevelFilter::Error,
        LevelFilter::Warn,
        LevelFilter::Info, // One --verbose give
        LevelFilter::Debug,
        LevelFilter::Trace,
    ]
        .get(cli_matches.occurrences_of("verbose") as usize + 2)
        .unwrap_or(&LevelFilter::Trace);

    // Logging
    let log_config = ConfigBuilder::new()
    // TODO: Filter time/thread by verbosity level instead of hard coding them off.
        // .set_time_level(LevelFilter::Off)
        // .set_thread_level(LevelFilter::Off)
        .set_target_level(*log_level_filter)
        // .add_filter_allow_str(
        //     crate_name!()
        //         .split_once("_")
        //         .map_or("transporter", |(a, _)| a),
        // )
        .build();

    CombinedLogger::init(vec![
        TermLogger::new(
            *log_level_filter,
            log_config.clone(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(
            *log_level_filter,
            log_config,
            std::fs::File::create(format!("{}.log", crate_name!())).unwrap(),
        ),
    ])
        .unwrap();
    log::info!("{} started", crate_name!());

    // Settings.
    let config_path = cli_matches.value_of_os("CONFIG_FILE").unwrap();
    let settings = Settings::new(config_path).unwrap();

    let https_connector = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https_connector);

    let samsara_config = Configuration {
        oauth_access_token: Some(settings.samsara.api_token),
        user_agent: Some(crate_name!().to_owned()),
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
            .flat_map(|resp| {
                let json_value = serde_json::to_value(&resp).expect("serialize result");
                let json_obj = json_value.as_object().expect("JSON object");
                let json_str = serde_json::to_string(&json_obj).unwrap();
                info!("vehicle stats response: {}", json_str);

                match json_obj["name"].as_str() {
                    Some(code) => {
                        json_obj
                            .iter()
                            .filter(|(_, value)| value.is_object())
                            .flat_map(|(kind, value)| {
                                let value_obj = value.as_object().unwrap();
                                let time = match value_obj["time"].as_str().map(|s| match s
                                    .parse::<DateTime<Utc>>()
                                {
                                    Ok(time) => time.naive_utc(),
                                    Err(e) => {
                                        error!(
                                            "Cannot parse time, using current time \"{:?}\": {:?}",
                                            json_obj["time]"], e
                                        );
                                        Utc::now().naive_utc()
                                    }
                                }) {
                                    Some(time) => time,
                                    None => {
                                        warn!("No time provided, using current time");
                                        Utc::now().naive_utc()
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
                            .collect()
                    }
                    _ => {
                        error!("Missing code in vehicle stat response");
                        Vec::new()
                    }
                }
            })
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

    // FIXME: Return the error and handle above.
    let postgres_config: Config = settings.transporter.database.into();
    match postgres_config.connect(NoTls).await {
        Ok((client, connection)) => {
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    eprintln!("connection error: {}", e);
                }
            });

            match client.prepare("INSERT INTO vehicle_stat (time, samsara_id, code, kind, json) \
                VALUES ($1, $2, $3, $4, $5) \
                ON CONFLICT (time, samsara_id, code, kind, json) DO NOTHING").await {
                Ok(insert_stmt) => {
                    trace!("Going to insert {:?} vehicle stats", db_insert_queue.len());
                    let mut retry_queue = VecDeque::new();
                    while let Some(stat) = db_insert_queue.pop_back() {
                        match client
                            .execute(&insert_stmt, &[&stat.time, &stat.samsara_id, &stat.code, &stat.kind, &stat.json])
                            .await {
                            Ok(row_count) =>
                                if row_count != 1 {
                                    error!("Inserted {} vehicle stat rows, expected 1", row_count)
                                }
                            Err(e) => {
                                retry_queue.push_front(stat);
                                error!("Cannot insert vehicle stat {:?}: {:?}", e, stat)
                            }
                        }
                    }
                    if !retry_queue.is_empty() {
                        trace!("There are {} vehicle stats in the retry queue", retry_queue.len());
                        db_insert_queue.extend(retry_queue);
                    }
                }
                Err(e) => error!("Cannot prepare statement: {:?}", e)
            }
        }
        Err(e) => error!("Cannot connect to the database: {:?}", e)
    };

    // {
    //     "engineState": {
    //     "time": "2021 - 07 - 13T20: 10: 23Z",w
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
    //     "time": "2021 - 07 - 14T02: 13: 53Z"
    //     },
    //     "id": "212014919004270",
    //     "name": "717",
    //     "obdOdometerMeters": {
    //     "time": "2021 - 07 - 13T20: 10: 22Z",
    //     "value": 386814875
    // }
    // }
    Ok(())
}
