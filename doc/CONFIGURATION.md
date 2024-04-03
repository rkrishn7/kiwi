# Kiwi Configuration

Kiwi can be configured using a YAML file. By default, Kiwi looks for configuration in `/etc/kiwi/config/kiwi.yml`. However, you may specify a different configuration file using the `--config` option CLI option.

The configuration format is described in detail below. Note that values shown are examples, and not defaults.

```yaml
# Hooks (plugins) are custom WASM modules that Kiwi loads and executes at various points
# in the event processing pipeline. Hooks are optional and may be used to extend Kiwi's
# functionality.
#
## Optional
hooks:
  # The authenticate hook is executed during the WebSocket handshake. It can be used to authenticate
  # clients using custom mechanisms and add context to the connection that will be passed downstream
  # to the intercept hook.
  #
  ## Optional (default: null)
  authenticate: 'my-authenticate-hook/target/wasm32-wasi/debug/authenticate_http.wasm'

  # This plugin executes once Kiwi ingest's the source event, but before it decides whether to
  # forward the event to any of the source's subscribers.
  #
  ## Optional (default: null)
  intercept: 'my-intercept-hook/target/wasm32-wasi/debug/intercept.wasm'

# Source Configuration
#
# Currently, Kiwi supports two types of sources: Kafka and Counter sources. Each source type
# is denoted by the `type` field and has its own set of required and optional fields.
#
## Required
sources:
  - type: kafka

    # The source ID for this counter source. The source ID is used as a unique identifier, thus must be
    # distinct from other source IDs, regardless of type.
    #
    ## Optional (defaults to `topic`)
    id: my-kafka-source

    # The topic name for this Kafka source. The source ID defaults to the topic name.
    #
    ## Required
    topic: 'my-topic'

  - type: counter

    # The source ID for this counter source. The source ID is used as a unique identifier, thus must be
    # distinct from other source IDs, regardless of type.
    #
    ## Required
    id: counter1

    # The interval at which the counter source emits events
    #
    ## Required
    interval_ms: 1000

    # Setting `lazy` to true will cause the counter source to start emitting events only upon its first
    # subscription.
    #
    ## Optional (default: false)
    lazy: true

    # The maximum value of the counter. Setting this value marks the source as a finite source. Once
    # the counter reaches this value, it will stop emitting events and disallow new subscriptions.
    #
    ## Optional (default: `u64::MAX`)
    max: 1000

    # The initial value of the counter
    #
    ## Required
    min: 0

# Kafka Consumer Configuration
#
## Required if any Kafka sources are defined. Optional otherwise
kafka:
  # The list of bootstrap servers to connect to
  #
  ## Required
  bootstrap_servers:
    - 'localhost:9092'

  # Whether to enable partition discovery. If enabled, Kiwi will periodically
  # query the Kafka cluster to discover new partitions.
  #
  ## Optional (default: true)
  partition_discovery_enabled: true

  # The interval at which to query the Kafka cluster to discover new partitions
  #
  ## Optional (default: 300000)
  partition_discovery_interval_ms: 300000

# WebSocket Server Configuration
#
## Required
server:
  # The address to bind the WebSocket server to
  #
  ## Required
  address: '127.0.0.1:8000'

  # Whether to enable the health check endpoint at `/health`
  #
  ## Optional (default: true)
  healthcheck: true

  # TLS Configuration
  #
  ## Optional
  tls:
    # The path to the certificate file
    #
    ## Required
    cert: '/path/to/cert.pem'
    # The path to the private key file
    #
    ## Required
    key: '/path/to/key.pem'


# Subscriber Configuration
#
# Subscribers are clients that connect to Kiwi and receive events from the sources
# they are subscribed to.
#
## Optional
subscriber:
  # For pull-based subscribers, Kiwi is capable of maintaining internal buffers to
  # handle backpressure. This setting controls the maximum number of events that may
  # be buffered for any given subscription. Once the capacity is reached, buffered events
  # will be dropped in a LRU fashion.
  # 
  # If this option is not set, Kiwi will drop events when a pull-based subscriber is 
  # unable to keep up with its respective source result frequency.
  #
  ## Optional (default: null)
  buffer_capacity: 100

  # The maximum number of events that a pull-based subscriber is permitted to
  # lag behind the source before Kiwi begins emitting lag notices. If not set,
  # Kiwi will not emit any lag notices.
  #
  ## Optional (default: null)
  lag_notice_threshold: 50
```
