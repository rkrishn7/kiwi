//! An example authenticate hook that parses an API key from the request query string
//! and makes a request to an API to verify the key

use http::request::Builder;
use kiwi_sdk::hook::authenticate::{authenticate, Outcome};
use kiwi_sdk::http::{request as http_request, Request};

/// You must use the `#[intercept]` macro to define an intercept hook.
#[authenticate]
fn handle(req: Request<()>) -> Outcome {
    let query = match req.uri().query() {
        Some(query) => query,
        // Returning `Outcome::Reject` instructs Kiwi to reject the connection
        None => return Outcome::Reject,
    };

    let parts: Vec<&str> = query.split('&').collect();

    // Parse the query string to find the API key
    // If the API key is not found, reject the connection
    let key = {
        let mut token = None;
        for (key, value) in parts.iter().map(|part| {
            let mut parts = part.split('=');
            (parts.next().unwrap(), parts.next().unwrap())
        }) {
            if key == "x-api-key" {
                token = Some(value);
                break;
            }
        }

        if let Some(token) = token {
            token
        } else {
            return Outcome::Reject;
        }
    };

    let request = Builder::new()
        .method("GET")
        .uri("https://example.com")
        .header("x-api-key", key)
        .body(Vec::new())
        .unwrap();

    // Make a request to the API to verify the API key
    match http_request(request) {
        Ok(res) => {
            if res.status() == 200 {
                // Returning `Outcome::Authenticate` instructs Kiwi to allow the connection to be established.
                Outcome::Authenticate
            } else {
                Outcome::Reject
            }
        }
        Err(_) => {
            Outcome::Reject
        }
    }
}
