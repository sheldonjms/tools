mod settings;

extern crate config;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate serde_derive;

use futures::TryFutureExt;
use hyper;
use hyper::Client;
use hyper_tls;
use hyper_tls::HttpsConnector;
use samsara::apis::AddressesApi;
use samsara::apis::configuration::Configuration;
use crate::settings::Settings;

// TODO: Move settings library to where the rest of the tools can see it.

#[tokio::main]
pub async fn main() -> std::io::Result<()> {
    let pkg_version = env!("CARGO_PKG_VERSION");
    let pkg_name = env!("CARGO_PKG_NAME");

    let settings = Settings::new().unwrap();
    println!("{:?}", settings);

    let https_connector = HttpsConnector::new();
    let client = Client::builder()
        .build::<_, hyper::Body>(https_connector);

    let samsara_config = Configuration {
        oauth_access_token: Some(settings.samsara.api_token),
        user_agent: Some(format!("{} {}", pkg_name, pkg_version)),
        ..Configuration::new(client)
    };
    let addressess_api = samsara::apis::AddressesApiClient::new(std::rc::Rc::new(samsara_config));
    let list_future = addressess_api.list_addresses(None, None, None, None, None);
//    list_future.and_then(|res| println!("RES: {:?}", res) );
    let x = list_future.await;
    println!("{:?}", x);

    //   rocket::build().mount("/api", routes![fleet])
    Ok(())
//}
}
