use kiwi_sdk::hook;
use kiwi_sdk::types::authenticate::Outcome;

#[hook::authenticate]
fn handle(_req: __kiwi_authenticate::preamble::wasi::http::types::IncomingRequest) -> Outcome {
    Outcome::Reject
}
