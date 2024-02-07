wasmtime::component::bindgen!({
    world: "authenticate-hook",
    path: "../wit",
    with: {
        "wasi:io/error": wasmtime_wasi::preview2::bindings::io::error,
        "wasi:io/streams": wasmtime_wasi::preview2::bindings::io::streams,
        "wasi:io/poll": wasmtime_wasi::preview2::bindings::io::poll,
        "wasi:clocks/monotonic-clock": wasmtime_wasi::preview2::bindings::clocks::monotonic_clock,
        "wasi:http/types": wasmtime_wasi_http::bindings::http::types,
        "wasi:http/outgoing-handler": wasmtime_wasi_http::bindings::http::outgoing_handler,
    }
});
