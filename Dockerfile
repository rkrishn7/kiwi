FROM rust:1.73.0-buster as builder
WORKDIR /usr/src

# Create a new empty shell project
RUN USER=root cargo new kiwi
WORKDIR /usr/src/kiwi

COPY ./Cargo.toml ./Cargo.toml
COPY ./Cargo.lock ./Cargo.lock

# Cache dependencies
RUN cargo build --release
RUN rm src/*.rs

COPY . .

# Build for release
RUN rm ./target/release/deps/kiwi*

RUN cargo build --release

FROM debian:buster-slim
WORKDIR /app

# Install libssl (Rust links against this library)
RUN apt-get update && \
    apt-get install -y libssl-dev ca-certificates && \
    apt-get clean

# Copy the binary from the builder stage
COPY --from=builder /usr/src/kiwi/target/release/kiwi .

CMD ["./kiwi"]
