# Kiwi Plugins

Kiwi allows operators to create WASM plugins that can be loaded at start-up to extend the functionality of the server.


## Creating a Plugin

A plugin is a WebAssembly module that implements a specific interface. Currently, Kiwi offers a Rust SDK to streamline the process of creating plugins

First, bootstrap a new plugin project using the `cargo` command-line tool:

```sh
cargo new --lib my-kiwi-plugin
```

Next, in the project folder, run the following command to add the `kiwi-sdk` crate:

```sh
cargo add kiwi-sdk
```

Before writing any code, ensure that crate type is specified as `cdylib` in the `Cargo.toml` file. This is necessary to compile a dynamic library that can be loaded by Kiwi at runtime.

```toml
[lib]
crate-type = ["cdylib"]
```

**Note**: The plugin must be built with the  `--target` flag set to `wasm32-wasi`

```sh
cargo build --target wasm32-wasi
```

Nice! You're ready to start writing your plugin. Take a look in the [examples](../examples) directory for Kiwi plugin samples.
