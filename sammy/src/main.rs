extern crate config;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate serde_derive;

use std::fs::File;
use chrono::prelude::*;
use futures::TryFutureExt;
use hyper;
use hyper::Client;
use hyper_tls;
use hyper_tls::HttpsConnector;
use log::{info, trace, warn};
use samsara::apis::{VehiclesApi, VehicleStatsApi};
use samsara::apis::configuration::Configuration;
use serde_json;
use simplelog::*;

use crate::settings::Settings;
use chrono::{DateTime, Utc};
use std::collections::{VecDeque, HashMap};
use serde_json::Value;

// TODO: Move settings library to where the rest of the tools can see it.
mod settings;

/// The responses from the Samsara API are logged to the console. Selection portions of the data
/// are written to the Transporter database.
#[tokio::main]
pub async fn main() -> std::io::Result<()> {
    let pkg_version = env!("CARGO_PKG_VERSION");
    let pkg_name = env!("CARGO_PKG_NAME");
    let app_name = format!("{} {}", pkg_name, pkg_version);

    // Logging
    let log_level = LevelFilter::Info;
    let log_config = simplelog::ConfigBuilder::new().set_time_format("%F %T".to_string()).build();
    let log_path = format!("{}.log", pkg_name);
    CombinedLogger::init(
        vec![
            TermLogger::new(log_level, log_config.clone(), TerminalMode::Mixed, ColorChoice::Auto),

            // FIXME: Does this overwrite the old log?
            WriteLogger::new(log_level, log_config.clone(), File::create(log_path).unwrap()),
        ]
    ).unwrap();
    log::info!("{} started", app_name);

    let settings = Settings::new().unwrap();

    let https_connector = HttpsConnector::new();
    let client = Client::builder()
        .build::<_, hyper::Body>(https_connector);

    let samsara_config = Configuration {
        oauth_access_token: Some(settings.samsara.api_token),
        user_agent: Some(format!("{} {}", pkg_name, pkg_version)),
        ..Configuration::new(client)
    };

    let vehicle_stats_api = samsara::apis::VehicleStatsApiClient::new(std::rc::Rc::new(samsara_config));
    let vehicle_stats_future = vehicle_stats_api.get_vehicle_stats(vec!("gps".to_string(), "engineStates".to_string(), "obdOdometerMeters".to_string()), None, None, None, None, None);

    // FIXME: Run once for now.
    // FIXME: Use locations feed for continual updates.
    // let vehicles_future = vehicles_api.list_vehicles(None, None, None, None);
    // fn get_equipment_locations(&self, after: Option<&str>, parent_tag_ids: Option<Vec<String>>, tag_ids: Option<Vec<String>>, equipment_ids: Option<Vec<String>>) -> Pin<Box<dyn Future<Output = Result<crate::models::EquipmentLocationsResponse, Error>>>>;

    // let addressess_api = samsara::apis::AddressesApiClient::new(std::rc::Rc::new(samsara_config));
    // let list_future = addressess_api.list_addresses(None, None, None, None, None);
//    list_future.and_then(|res| println!("RES: {:?}", res) );
//     let x = vehicles_future.await;

    #[derive(Debug)]
    struct VehicleStat {
        time: DateTime<Utc>,
        samsara_id: Option<String>,
        code: String,
        kind: String,
        json: String,
    }

    // Items to insert into the database. Reduces the chance of losing items when
    // the database is down.
    const DB_INSERT_QUEUE_MAX: usize = 1000000;
    // let db_insert_queue = VecDeque::new();

    match vehicle_stats_future.await {
        Ok(response) => response.data.iter().for_each(|resp| {
            let json_value = serde_json::to_value(&resp).expect("serialize result");
            let json_obj = json_value.as_object().expect("JSON object");
            let json_str = serde_json::to_string(&json_obj).unwrap();
            info!("vehicle stats response: {}", json_str);

            match json_obj["name"].as_str() {
                Some(code) => {
                    let vehicle_stats = json_obj.iter()
                        .filter(|(_, value)| value.is_object())
                        .map(|(kind, value)| {
                            let value_obj = value.as_object().unwrap();
                            let time = match value_obj["time"].as_str().map(|s| match s.parse::<DateTime<Utc>>() {
                                Ok(time) => time,
                                Err(e) => {
                                    error!("Cannot parse time, using current time: {:?}", json_obj["time]"]);
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
                            value_obj.iter().for_each(|(key, value)|
                                if key != "time" {
                                    pruned_values.insert(key.clone(), value.clone());
                                });

                            VehicleStat {
                                time,
                                samsara_id: json_obj["id"].as_str().map(|s| s.to_string()),
                                code: code.to_string(),
                                kind: kind.clone(),
                                json: serde_json::to_string(&pruned_values).unwrap(),
                            }
                        });
                    vehicle_stats.for_each(|stat| println!("STAT: {:?}", stat));
                }
                _ => error!("Missing code in vehicle stat response")
            }
        }),
        Err(e) => error!("Cannot retrieve vehicle stats: {:?}", e)
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
