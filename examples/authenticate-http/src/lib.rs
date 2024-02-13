use kiwi_sdk::hook;
use kiwi_sdk::types::authenticate::Outcome;
use kiwi_sdk::wasi;

wasi::use_wasi_http_types!(http_types);
wasi::http::make_http_request_fn!(http_request);

#[hook::authenticate]
fn handle(req: http_types::IncomingRequest) -> Outcome {
    let path_with_query = if let Some(path_with_query) = req.path_with_query() {
        path_with_query
    } else {
        return Outcome::Reject;
    };

    let uri: http::Uri = match path_with_query.parse() {
        Ok(uri) => uri,
        Err(_) => return Outcome::Reject,
    };

    let query = match uri.query() {
        Some(query) => query,
        None => return Outcome::Reject,
    };

    let parts: Vec<&str> = query.split('&').collect();

    let token = {
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

    println!("token: {}", token);

    match http_request(
        http_types::Method::Get,
        http_types::Scheme::Https,
        "google.com",
        "/",
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
