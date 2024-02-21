use http::request::Builder;
use kiwi_sdk::hook::authenticate::{authenticate, Outcome};
use kiwi_sdk::http::{request as http_request, Request};

#[authenticate]
fn handle(req: Request<()>) -> Outcome {
    let query = match req.uri().query() {
        Some(query) => query,
        None => return Outcome::Reject,
    };

    let parts: Vec<&str> = query.split('&').collect();

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

    println!("Received API Key: {key}");

    let request = Builder::new()
        .method("GET")
        .uri("https://example.com")
        .header("x-api-key", key)
        .body(Vec::new())
        .unwrap();

    match http_request(request) {
        Ok(res) => {
            if res.status() == 200 {
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
