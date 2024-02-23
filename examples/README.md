# Examples of running Kiwi 🥝

This directory contains examples of running Kiwi in various configurations. Each example is self-contained and includes a `README.md` file with instructions on how to run it.

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) - The Rust toolchain is required to build WASM hooks.
- [Docker](https://docs.docker.com/get-docker/) - Docker is utilized in each example as a simple way to run Kiwi.
- [npm](https://docs.npmjs.com/downloading-and-installing-node-js-and-npm) - The Node.js package manager is required to install [wscat](https://www.npmjs.com/package/wscat), a WebSocket client used to interact with the Kiwi server.

## Examples

- [Intercept (Simple)](./intercept-simple): A simple example that demonstrates how to write WASM hooks using the Kiwi SDK and load them into the Kiwi runtime.
- [Authenticate w/ Outbound HTTP](./authenticate-http): An example that demonstrates how to make outbound HTTP requests in the authenticate hook using the Kiwi SDK.
