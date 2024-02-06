FROM lukemathwalker/cargo-chef:latest-rust-slim-bookworm AS chef
WORKDIR /app

FROM chef AS planner

COPY . .

RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

COPY --from=planner /app/recipe.json recipe.json

# Install cmake (For building librdkafka)
RUN apt-get update && \
    apt-get install -y cmake curl g++ && \
    apt-get clean

# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json

# Build application
COPY . .
RUN cargo build --release --locked --bin kiwi

# Install the necessary WASI adapter module for the Kiwi hook runtime
RUN curl -L https://github.com/bytecodealliance/wasmtime/releases/download/v17.0.0/wasi_snapshot_preview1.reactor.wasm \
    -o wasi_snapshot_preview1.wasm

FROM debian:bookworm-slim
WORKDIR /app

# Install libssl (Rust links against this library)
RUN apt-get update && \
    apt-get install -y libssl-dev ca-certificates && \
    apt-get clean

COPY --from=builder /app/target/release/kiwi /usr/local/bin
COPY --from=builder /app/wasi_snapshot_preview1.wasm /etc/kiwi/wasi/wasi_snapshot_preview1.wasm

ENTRYPOINT ["/usr/local/bin/kiwi"]
