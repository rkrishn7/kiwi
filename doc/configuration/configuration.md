# Kiwi configuration Schema

```txt
undefined
```



| Abstract            | Extensible | Status         | Identifiable | Custom Properties | Additional Properties | Access Restrictions | Defined In                                                                    |
| :------------------ | :--------- | :------------- | :----------- | :---------------- | :-------------------- | :------------------ | :---------------------------------------------------------------------------- |
| Can be instantiated | No         | Unknown status | No           | Forbidden         | Allowed               | none                | [configuration.schema.json](configuration.schema.json "open original schema") |

## Kiwi configuration Type

`object` ([Kiwi configuration](configuration.md))

# Kiwi configuration Properties

| Property                  | Type     | Required | Nullable       | Defined by                                                                                                    |
| :------------------------ | :------- | :------- | :------------- | :------------------------------------------------------------------------------------------------------------ |
| [hooks](#hooks)           | `object` | Optional | cannot be null | [Kiwi configuration](configuration-properties-hook-configuration.md "undefined#/properties/hooks")            |
| [sources](#sources)       | `array`  | Required | cannot be null | [Kiwi configuration](configuration-properties-source-configuration.md "undefined#/properties/sources")        |
| [server](#server)         | `object` | Required | cannot be null | [Kiwi configuration](configuration-properties-server-configuration.md "undefined#/properties/server")         |
| [subscriber](#subscriber) | `object` | Optional | cannot be null | [Kiwi configuration](configuration-properties-subscriber-configuration.md "undefined#/properties/subscriber") |

## hooks

WebAssembly hooks

`hooks`

*   is optional

*   Type: `object` ([Hook configuration](configuration-properties-hook-configuration.md))

*   cannot be null

*   defined in: [Kiwi configuration](configuration-properties-hook-configuration.md "undefined#/properties/hooks")

### hooks Type

`object` ([Hook configuration](configuration-properties-hook-configuration.md))

## sources

Sources

`sources`

*   is required

*   Type: an array of merged types ([Details](configuration-properties-source-configuration-items.md))

*   cannot be null

*   defined in: [Kiwi configuration](configuration-properties-source-configuration.md "undefined#/properties/sources")

### sources Type

an array of merged types ([Details](configuration-properties-source-configuration-items.md))

## server

Server configuration

`server`

*   is required

*   Type: `object` ([Server configuration](configuration-properties-server-configuration.md))

*   cannot be null

*   defined in: [Kiwi configuration](configuration-properties-server-configuration.md "undefined#/properties/server")

### server Type

`object` ([Server configuration](configuration-properties-server-configuration.md))

## subscriber

Subscriber configuration

`subscriber`

*   is optional

*   Type: `object` ([Subscriber configuration](configuration-properties-subscriber-configuration.md))

*   cannot be null

*   defined in: [Kiwi configuration](configuration-properties-subscriber-configuration.md "undefined#/properties/subscriber")

### subscriber Type

`object` ([Subscriber configuration](configuration-properties-subscriber-configuration.md))
