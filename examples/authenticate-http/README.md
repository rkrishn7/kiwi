# Authenticate w/ Outbound HTTP

This example demonstrates how to make outbound HTTP requests in the authenticate hook using the Kiwi SDK.

- [Authenticate w/ Outbound HTTP](#authenticate-w-outbound-http)
  - [Running the Example](#running-the-example)
    - [Building the WASM Hook](#building-the-wasm-hook)
    - [Running Kiwi](#running-kiwi)
    - [Establishing a Connection](#establishing-a-connection)
  - [Recap](#recap)

## Running the Example

> **NOTE**: The commands in this example should be run from this directory (`examples/authenticate-http`).

### Building the WASM Hook

The `wasm32-wasip1` target is required to build the WASM hook. This target is not installed by default, so it must be added using the following command:

```sh
rustup target add wasm32-wasip1
```

Once the target is installed, the WASM hook can be built using the following command:

```sh
cargo build --target wasm32-wasip1
```

This command will produce the WASM hook at `target/wasm32-wasip1/debug/authenticate_http.wasm`.

### Running Kiwi

Now that the WASM hook is built, it can be run with Kiwi. The following command will run Kiwi with the WASM hook and the provided configuration file:

```sh
docker run -p 8000:8000 -v $(pwd)/kiwi.yml:/etc/kiwi/config/kiwi.yml \
    -v $(pwd)/target/wasm32-wasip1/debug/authenticate_http.wasm:/etc/kiwi/hook/authenticate.wasm \
    ghcr.io/rkrishn7/kiwi:main
```

### Establishing a Connection

Now we can interact with the Kiwi server at `ws://localhost:8000`. Let's try it out by subscribing to a counter source and emitting some events. First, let's try to connect to the server using `wscat`, without providing an `x-api-key` query parameter:

```sh
wscat -c ws://127.0.0.1:8000
```

The connection should be rejected by the server. Now, let's try to connect to the server, making sure to provide an `x-api-key` query parameter:

```sh
wscat -c ws://127.0.0.1:8000/some-path?x-api-key=secret
```

Success! The connection should be accepted by the server. Behind the scenes, your WASM hook is making an outbound HTTP request to a mock authentication server to verify the `x-api-key` query parameter.

## Recap

This example demonstrated how to write an authentication hook using the Kiwi SDK and load it into Kiwi for execution. The hook makes an outbound HTTP request to a mock authentication server to verify the `x-api-key` query parameter.

Real-world use cases for this type of hook include verifying JWT tokens, checking for specific user roles, and more.
