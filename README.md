# 🥝 Kiwi

[![test](https://github.com/rkrishn7/kiwi/actions/workflows/test.yml/badge.svg)](https://github.com/rkrishn7/kiwi/actions/workflows/test.yml) [![check](https://github.com/rkrishn7/kiwi/actions/workflows/check.yml/badge.svg)](https://github.com/rkrishn7/kiwi/actions/workflows/check.yml) [![codecov](https://codecov.io/gh/rkrishn7/kiwi/graph/badge.svg?token=G6QRFUHNM4)](https://codecov.io/gh/rkrishn7/kiwi)

Kiwi is a WebSocket gateway for real-time event sources. It implements a simple protocol for clients to subscribe to configured sources, ensuring that they stay reactive and up-to-date with the latest data.

***NOTE***: Kiwi is currently in active development and is not yet ready for production use.

## Features

- **Subscribe with Ease**: Set up subscriptions to various sources with a simple command. Kiwi efficiently routes event data to connected WebSocket clients based on these subscriptions.
- **Backpressure Management**: Kiwi draws from flow-control concepts used by Reactive Streams. Specifically, clients can emit a `request(n)` signal to control the rate at which they receive events.
- **Extensible**: Kiwi supports WebAssembly (WASM) plugins to enrich and control the flow of data. Plugins are called with context about the current connection and event, and can be used to control how/when events are forwarded to downstream clients.
- **Secure**: Kiwi supports TLS encryption and client authentication via JWT tokens, which can be used to authorize access to specific sources.
- **Configuration Reloads**: Kiwi can reload a subset of its configuration at runtime, allowing for dynamic updates to sources without restarting the server.

## Motivation

The digital era has increasingly moved towards real-time data and event-driven architectures. Tools like Apache Kafka have set the standard for robust, high-throughput messaging and streaming, enabling applications to process and react to data as it arrives. Kafka, and the ecosystem built around it, excel at ingesting streams of events and providing the backbone for enterprise-level data processing and analytics. However, there's often a disconnect when trying to extend this real-time data paradigm directly to end-users in a web or mobile environment.

Enter **Kiwi**.

While Kafka and technologies that build upon it serve as powerful platforms for data aggregation and processing, Kiwi aims to complement these tools by acting as the last mile for delivering real-time data to users. Serving as a "general-purpose" gateway, a major component of Kiwi is its plugin interface, empowering developers to define the behavior of their plugins according to the unique requirements of their applications. This approach allows Kiwi to focus on its primary objective of efficiently routing data to clients and managing subscriptions.

## Getting Started

TODO


## Considerations

Kiwi is designed as a real-time event notification service, leveraging WebAssembly (WASM) plugins to enrich and control the flow of data. While Kiwi supports certain operations commonly associated with stream processing, such as map and filter, it is not intended to replace full-fledged stream processing frameworks.

Kiwi excels at handling event-driven communication with efficient backpressure management, making it suitable for real-time messaging and lightweight data transformation tasks. However, users requiring advanced stream processing capabilities—such as complex event processing (CEP), stateful computations, windowing, and aggregation over unbounded datasets—are encouraged to use specialized stream processing systems.

Kiwi is designed to be a part of a broader architecture where it can work in conjunction with such systems, rather than serve as a standalone solution for high-throughput data processing needs.
