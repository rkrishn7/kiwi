# Kiwi Protocol

Kiwi uses a simple, text-based protocol to allow clients to interact with the server. Acting as a WebSocket gateway for real-time event sources, Kiwi is designed to be language-agnostic and can be used with any WebSocket client library. The protocol is implemented in JSON and intended to be simple and easy to understand.

In short, the protocol consists of a set of commands that clients can issue, which in turn trigger responses from the server. Additionally, the server may, at any time, send asynchronous messages to the client. Typically, these messages are events from subscribed sources, but they can also be error messages or other notifications.

- [Kiwi Protocol](#kiwi-protocol)
  - [Subscriptions](#subscriptions)
    - [Subscribing to Sources](#subscribing-to-sources)
    - [Unsubscribing from Sources](#unsubscribing-from-sources)
    - [Requesting Events (Pull-Based Subscriptions)](#requesting-events-pull-based-subscriptions)
    - [Subscription Results](#subscription-results)
  - [Notices](#notices)
    - [Lag Notices](#lag-notices)
    - [Subscription Closed Notices](#subscription-closed-notices)

## Subscriptions

### Subscribing to Sources

Clients can subscribe to sources by sending a `SUBSCRIBE` command to the server:

```json
{
  "type": "SUBSCRIBE",
  "sourceId": string,
  // Optional (defaults to "push")
  "mode": "pull" | "push"
}
```

In the payload schema above, `sourceId` is the unique identifier of the defined source in the server's configuration. The `mode` field is optional and defaults to `push`. When set to `pull`, the server will not send events to the client until the client explicitly requests them using the [`REQUEST` command](#requesting-events-pull-based-subscriptions). This mode is useful for clients that want to have control over the rate at which they receive events.

Upon successful subscription, the server will respond with a `SUBSCRIBE_OK` command response:

```json
{
  "type": "SUBSCRIBE_OK",
  "data": {
    "sourceId": string
  }
}
```

Here, the `sourceId` field will match the `sourceId` from the original `SUBSCRIBE` command.

If the subscription fails, the server will respond with a `SUBSCRIBE_ERROR` command:

```json
{
  "type": "SUBSCRIBE_ERROR",
  "data": {
    "sourceId": string,
    "error": string
  }
}
```

The `error` field will contain a human-readable error message explaining why the subscription failed.

### Unsubscribing from Sources

Clients can unsubscribe from subscribed sources by sending an `UNSUBSCRIBE` command to the server:

```json
{
  "type": "UNSUBSCRIBE",
  "sourceId": string
}
```

The `sourceId` specified above must be associated with an active subscription maintained by the server for the client, otherwise the server will respond with an `UNSUBSCRIBE_ERROR` command:

```json
{
  "type": "UNSUBSCRIBE_ERROR",
  "data": {
    "sourceId": string,
    "error": string
  }
}
```

Upon successful unsubscription, the server will respond with an `UNSUBSCRIBE_OK` command:

```json
{
  "type": "UNSUBSCRIBE_OK",
  "data": {
    "sourceId": string
  }
}
```

### Requesting Events (Pull-Based Subscriptions)

Clients can request events from the server for pull-based subscriptions by sending a `REQUEST` command:

```json
{
  "type": "REQUEST",
  "sourceId": string,
  "n": number
}
```

The `sourceId` field must match the `sourceId` of an active pull-based subscription. The `n` field specifies the number of events the client is requesting from the server.

> NOTE: `REQUEST` commands are additive in that they do not replace the previous request. Instead, the server will accumulate the number of requested events across multiple `REQUEST` commands and send events to the client accordingly.

Upon successful request, the server will respond with a `REQUEST_OK` command:

```json
{
  "type": "REQUEST_OK",
  "data": {
    "sourceId": string,
    "n": number
  }
}
```

If the request fails, the server will respond with a `REQUEST_ERROR` command:

```json
{
  "type": "REQUEST_ERROR",
  "data" {
    "sourceId": string,
    "error": string
  }
}
```

### Subscription Results

The server will issue `RESULT` messages to the client when events are available for any subscribed sources. The payload of a `RESULT` message will contain the event data:

```json
{
  "type": "RESULT",
  "data": SourceData
}
```

Where `SourceData` is a source-specific data structure that contains the event payload, source ID, and any other relevant metadata. The type of `SourceData` is represented as the following:

```ts
type SourceData = KafkaSourceData | CounterSourceData;

type KafkaSourceData = {
  sourceId: string,
  sourceType: "kafka",
  // The payload is base64 encoded
  payload: string,
  partition: number,
  offset: number,
  key: string,
  value: string,
  timestamp?: number
};

type CounterSourceData = {
  sourceId: string,
  sourceType: "counter",
  count: number
};
```

## Notices

The server may send notices to the client at any time. These notices can be informational, error messages.

### Lag Notices

When a client is subscribed to a Kafka source, the server may send lag notices to the client. Lag most commonly may occur during pull-based subscriptions, where the client is not consuming events as fast as they are being produced and the buffer capacity has been reached. A lag notice looks like this:

```json
{
  "type": "NOTICE",
  "data": {
    "type": "LAG",
    "sourceId": string,
    "count": number,
  }
}
```

Here, `count` is the number of events that the client is lagging behind. The `sourceId` field will match the `sourceId` of the source for which the lag notice is being sent.

### Subscription Closed Notices

If a subscription is closed by the server not due to an explicit unsubscription request from the client, the server will send a subscription closed notice to the client:

```json
{
  "type": "NOTICE",
  "data": {
    "type": "SUBSCRIPTION_CLOSED",
    "sourceId": string,
    "message": string
  }
}
```

The `sourceId` field will match the `sourceId` of the source for which the subscription was closed. The `message` field will contain a human-readable message explaining why the subscription was closed.

Subscriptions may close due to various reasons, such as the source ending (for finite sources), the source metadata changing, the source being deleted in the configuration, or some server error.
