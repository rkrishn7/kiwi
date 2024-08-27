wasmtime::component::bindgen!({
    world: "authenticate-hook",
    path: "../wit",
    async: true,
    tracing: true,
    with: {
        "wasi:io/error": wasmtime_wasi::bindings::io::error,
        "wasi:io/streams": wasmtime_wasi::bindings::io::streams,
        "wasi:io/poll": wasmtime_wasi::bindings::io::poll,
        "wasi:clocks/monotonic-clock": wasmtime_wasi::bindings::clocks::monotonic_clock,
        "wasi:http/types": wasmtime_wasi_http::bindings::http::types,
        "wasi:http/outgoing-handler": wasmtime_wasi_http::bindings::http::outgoing_handler,
    }
});
