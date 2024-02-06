# Intercept (Simple)

This example highlights how to write a simple WebAssembly (WASM) hook using the Kiwi SDK and load it into the Kiwi runtime. The hook is a simple plugin that intercepts all all events emitted by counter sources and discards them if they are odd.

- [Intercept (Simple)](#intercept-simple)
  - [Running the Example](#running-the-example)
    - [Prerequisites](#prerequisites)
    - [Building the WASM Hook](#building-the-wasm-hook)
    - [Running Kiwi](#running-kiwi)
    - [Interacting with Kiwi](#interacting-with-kiwi)
  - [Recap](#recap)

## Running the Example

> **NOTE**: The commands in this example should be run from this directory (`examples/intercept-simple`).

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) - The Rust toolchain is required to build the WASM hook.
- [Docker](https://docs.docker.com/get-docker/) - Docker is utilized in this example as a simple way to run Kiwi.
- [npm](https://docs.npmjs.com/downloading-and-installing-node-js-and-npm) - The Node.js package manager is required to install [wscat](https://www.npmjs.com/package/wscat), a WebSocket client used to interact with the Kiwi server.

### Building the WASM Hook

The `wasm32-wasi` target is required to build the WASM hook. This target is not installed by default, so it must be added using the following command:

```sh
rustup target add wasm32-wasi
```

Once the target is installed, the WASM hook can be built using the following command:

```sh
cargo build --target wasm32-wasi
```

This command will produce the WASM hook at `target/wasm32-wasi/debug/intercept.wasm`.

### Running Kiwi

Now that the WASM hook is built, it can be run with Kiwi. The following command will run Kiwi with the WASM hook and the provided configuration file:

```sh
docker run -p 8000:8000 -v $(pwd)/kiwi.yml:/etc/kiwi/config/kiwi.yml \
    -v $(pwd)/target/wasm32-wasi/debug/intercept.wasm:/etc/kiwi/hook/intercept.wasm \
    ghcr.io/rkrishn7/kiwi:main
```

### Interacting with Kiwi

The Kiwi server is now running with the WASM hook loaded. To interact with it, let's go ahead and install [wscat](https://github.com/websockets/wscat):

```sh
npm install -g wscat
```

Awesome! Now we can interact with the Kiwi server at `http://localhost:8000`. Let's try it out by subscribing to a counter source and emitting some events. First, let's connect to the server using `wscat`:

```sh
wscat -c ws://127.0.0.1:8000
```

Now that we're connected, let's subscribe to the counter source. In the `wscat` terminal, send the following message:

```json
{"type":"SUBSCRIBE","sourceId":"counter1"}
```

The server should respond with a message indicating that the subscription was successful. After this, you should start receiving events from the counter source. Note that even though the counter source is emitting all the natural numbers, the WASM hook is intercepting and discarding all odd numbers. Thus, you only see the even numbers in the terminal!

## Recap

This example demonstrated how to write a simple WebAssembly (WASM) hook using the Kiwi SDK and load it into Kiwi for execution.
