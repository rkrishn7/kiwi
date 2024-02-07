use kiwi_sdk::hook;
use kiwi_sdk::types::authenticate::Outcome;
use kiwi_sdk::types::http::IncomingRequest;

#[hook::authenticate]
fn handle(_req: IncomingRequest) -> Outcome {
    Outcome::Authenticate
}
