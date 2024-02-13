wit_bindgen::generate!(
    world: "authenticate-hook",
    exports: {
        world: Kiwi,
    },
    runtime_path: "::kiwi_sdk::wit_bindgen::rt",
    with: {
        "wasi:io/error": ::kiwi_sdk::wit::wasi::io::error,
        "wasi:io/streams": ::kiwi_sdk::wit::wasi::io::streams,
        "wasi:io/poll": ::kiwi_sdk::wit::wasi::io::poll,
        "wasi:clocks/monotonic-clock": ::kiwi_sdk::wit::wasi::clocks::monotonic_clock,
        "wasi:http/types": ::kiwi_sdk::wit::wasi::http::types,
        "wasi:http/outgoing-handler": ::kiwi_sdk::wit::wasi::http::outgoing_handler,
    },
    path: "../../wit",
);

// wit_bindgen::generate!("authenticate-hook" in "../../wit");
