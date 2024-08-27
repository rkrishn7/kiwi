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

FROM debian:bookworm-slim
WORKDIR /app

# Install libssl (Rust links against this library)
RUN apt-get update && \
    apt-get install -y libssl-dev ca-certificates && \
    apt-get clean

COPY --from=builder /app/target/release/kiwi /usr/local/bin

ENTRYPOINT ["/usr/local/bin/kiwi"]
