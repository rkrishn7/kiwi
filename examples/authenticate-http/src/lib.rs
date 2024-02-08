use anyhow::{anyhow, Result};
use kiwi_sdk::hook;
use kiwi_sdk::types::authenticate::Outcome;
use kiwi_sdk::wasi;

wasi::use_wasi_http_types!();
wasi::http::make_http_request_fn!(http_request);

#[hook::authenticate]
fn handle(_req: http_types::IncomingRequest) -> Outcome {
    match http_request(
        http_types::Method::Get,
        http_types::Scheme::Https,
        "api.joinfound.com",
        "/healthz",
        None,
        None,
    ) {
        Ok(res) => {
            if std::str::from_utf8(&res.body).unwrap() == "OK" {
                Outcome::Authenticate
            } else {
                Outcome::Reject
            }
        }
        Err(err) => {
            println!("error: {:?}", err);
            Outcome::Reject
        }
    }
}
