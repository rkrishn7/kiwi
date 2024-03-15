# 🥝 Kiwi - Extensible Real-Time Data Streaming

[![test](https://github.com/rkrishn7/kiwi/actions/workflows/test.yml/badge.svg)](https://github.com/rkrishn7/kiwi/actions/workflows/test.yml) [![check](https://github.com/rkrishn7/kiwi/actions/workflows/check.yml/badge.svg)](https://github.com/rkrishn7/kiwi/actions/workflows/check.yml) [![CircleCI](https://dl.circleci.com/status-badge/img/gh/rkrishn7/kiwi/tree/main.svg?style=shield)](https://dl.circleci.com/status-badge/redirect/gh/rkrishn7/kiwi/tree/main) ![contributions](https://img.shields.io/badge/contributions-welcome-green)

Kiwi is a WebSocket adapter for real-time data sources. It implements a simple protocol for clients to subscribe to configured sources, while allowing operators to maintain control over the flow of data via [WebAssembly](https://webassembly.org/) (WASM) plugins. Kiwi is designed to be a lightweight, extensible, and secure solution for delivering real-time data to clients, ensuring that they stay reactive and up-to-date with the latest data.

***NOTE***: Kiwi is currently in active development and is not yet recommended for production use.

- [🥝 Kiwi - Extensible Real-Time Data Streaming](#-kiwi---extensible-real-time-data-streaming)
  - [Features](#features)
  - [Supported Sources](#supported-sources)
    - [Kafka](#kafka)
    - [Counter](#counter)
  - [Motivation](#motivation)
  - [Getting Started](#getting-started)
  - [Plugins](#plugins)
  - [Protocol](#protocol)
  - [Configuration](#configuration)
  - [Considerations](#considerations)

## Features

- **Subscribe with Ease**: Set up subscriptions to various sources with a simple command. Kiwi efficiently routes event data to connected WebSocket clients based on these subscriptions.
- **Extensible**: Kiwi supports WebAssembly (WASM) plugins to enrich and control the flow of data. Plugins are called with context about the current connection and event, and can be used to control how/when events are forwarded to downstream clients.
- **Backpressure Management**: Kiwi draws from flow-control concepts used by Reactive Streams. Specifically, clients can emit a `request(n)` signal to control the rate at which they receive events.
- **Secure**: Kiwi supports TLS encryption and custom client authentication via WASM plugins.
- **Configuration Reloads**: Kiwi can reload a subset of its configuration at runtime, allowing for dynamic updates to sources and plugin code without restarting the server.

## Supported Sources

### Kafka

Currently, Kiwi primarily supports Kafka as a data source, with plans to support additional sources in the future. Kafka sources are backed by a high-performance Rust Kafka client, [rust-rdkafka](https://github.com/fede1024/rust-rdkafka), and support automatic partition discovery.

Notably, Kiwi does not leverage balanced consumer groups for Kafka sources. Instead, it subscribes to the entire set of partitions for a given topic and invokes the configured intercept plugin, if any, for each event. This has a few implications:

- Kiwi may not be suitable for very high-throughput Kafka topics, as the freshness of events may be impacted by the combination of the high volume of events across partitions and per-event processing time. There are plans to support a deterministic partitioning plugin for clients in the future to address this limitation for supported use cases.
- Event processing cannot be parallelized across multiple instances of Kiwi. If vertical scaling is not sufficient to handle the combined throughput of all configured sources, Kiwi may not be the best fit.

### Counter

Kiwi also includes a simple counter source for testing and demonstration purposes. The counter source emits a monotonically increasing integer at a configurable interval, and is primarily used to demonstrate the behavior of Kiwi with a simple source.

## Motivation

The digital era has increasingly moved towards real-time data and event-driven architectures. Tools like Apache Kafka have set the standard for robust, high-throughput messaging and streaming, enabling applications to process and react to data as it arrives. Kafka, and the ecosystem built around it, excel at ingesting streams of events and providing the backbone for enterprise-level data processing and analytics. However, there's often a disconnect when trying to extend this real-time data paradigm directly to end-users in a web or mobile environment.

Enter **Kiwi**.

While Kafka and technologies that build upon it serve as powerful platforms for data aggregation and processing, Kiwi aims to complement these tools by acting as the last mile for delivering real-time data to users. Serving as a "general-purpose" gateway, a major component of Kiwi is its plugin interface, empowering developers to define the behavior of their plugins according to the unique requirements of their applications. This approach allows Kiwi to focus on its primary objective of efficiently routing data to clients and managing subscriptions.

## Getting Started

The easiest way to get started with Kiwi is with Docker! First, create a simple configuration file for Kiwi named `kiwi.yml`:

```yaml
sources:
  # Counter sources are primarily used for testing and demonstration purposes
  - id: "counter"
    type: "counter"
    interval_ms: 1000
    lazy: true
    min: 0

server:
  address: '0.0.0.0:8000'
```

Next, in the same directory as the `kiwi.yml` file, run the following command to start Kiwi:

```sh
docker run -p 8000:8000 -v $(pwd)/kiwi.yml:/etc/kiwi/config/kiwi.yml ghcr.io/rkrishn7/kiwi:main
```

Success! Kiwi is now running and ready to accept WebSocket connections on port 8000. You can start interacting with the server by using a WebSocket client utility of your choice (e.g. [wscat](https://www.npmjs.com/package/wscat)). Refer to the [protocol documentation](./doc/PROTOCOL.md) for details on how to interact with the Kiwi server.

For more examples, please see the [examples](./examples) directory.

## Plugins

Kiwi supports WebAssembly (WASM) plugins which allows developers to define the behavior of event delivery and authorization according to the unique requirements of their applications. A [Rust SDK](https://docs.rs/kiwi-sdk/latest/kiwi_sdk/) is provided to simplify the process of writing plugins in Rust.

There are two types of plugins that Kiwi supports:

- **Intercept**: Intercept plugins are invoked before an event is sent to a client. They are called with context about the current connection and event, and can be used to control how/when events are forwarded to downstream clients.
  - For example, imagine you are writing a chat application and only want users to receive messages they are authorized to see. While the chat message source may emit messages for all conversations, an intercept plugin can be used to filter out messages that the user is not authorized to see.

- **Authentication**: Authentication plugins are invoked when a client connects to the server. They are called with context about the current connection and can be used to authenticate the client, potentially rejecting the connection if the client is not authorized to connect.
  - Authentication plugins allow users of Kiwi to enforce custom authentication logic, such as verifying JWT tokens or checking for specific user roles. Additionally, the plugin may return custom context for the connection which is passed downstream to each invocation of the intercept plugin.

For more information on writing and using plugins, please see the [plugin documentation](./doc/PLUGIN.md).

## Protocol

Details on the Kiwi protocol can be found in the [protocol documentation](./doc/PROTOCOL.md).

## Configuration

Details on configuring Kiwi can be found in the [configuration documentation](./doc/CONFIGURATION.md).

## Considerations

Kiwi is designed as a real-time event notification service, leveraging WebAssembly (WASM) plugins to enrich and control the flow of data. While Kiwi supports certain operations commonly associated with stream processing, such as map and filter, it is not intended to replace full-fledged stream processing frameworks.

Kiwi excels at handling event-driven communication with efficient backpressure management, making it suitable for real-time messaging and lightweight data transformation tasks. However, users requiring advanced stream processing capabilities—such as complex event processing (CEP), stateful computations, windowing, and aggregation over unbounded datasets—are encouraged to use specialized stream processing systems.

Kiwi is designed to be a part of a broader architecture where it can work in conjunction with such systems, rather than serve as a standalone solution for high-throughput data processing needs.

