# Common flag to remap WASI imports to their respective Kiwi SDK counterparts
wasi_remaps := '--with "wasi:http/outgoing-handler@0.2.0=::kiwi_sdk::wit::wasi::http::outgoing_handler,wasi:http/types@0.2.0=::kiwi_sdk::wit::wasi::http::types,wasi:clocks/monotonic-clock@0.2.0=::kiwi_sdk::wit::wasi::clocks::monotonic_clock,wasi:io/poll@0.2.0=::kiwi_sdk::wit::wasi::io::poll,wasi:io/streams@0.2.0=::kiwi_sdk::wit::wasi::io::streams,wasi:io/error@0.2.0=::kiwi_sdk::wit::wasi::io::error"'

# Generates the expanded guest code for the authenticate hook
wit-bindgen-authenticate-hook:
    wit-bindgen rust ./src/wit --world authenticate-hook --exports 'world=Kiwi' --out-dir ./src/kiwi-sdk/macro/src --runtime-path '::kiwi_sdk::wit_bindgen::rt' {{ wasi_remaps }}

# Generates the expanded guest code for the intercept hook
wit-bindgen-intercept-hook:
    wit-bindgen rust ./src/wit --world intercept-hook --exports 'world=Kiwi' --out-dir ./src/kiwi-sdk/macro/src --runtime-path '::kiwi_sdk::wit_bindgen::rt'
