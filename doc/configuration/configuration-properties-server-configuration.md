# Server configuration Schema

```txt
undefined#/properties/server
```

Server configuration

| Abstract            | Extensible | Status         | Identifiable | Custom Properties | Additional Properties | Access Restrictions | Defined In                                                                      |
| :------------------ | :--------- | :------------- | :----------- | :---------------- | :-------------------- | :------------------ | :------------------------------------------------------------------------------ |
| Can be instantiated | No         | Unknown status | No           | Forbidden         | Allowed               | none                | [configuration.schema.json\*](configuration.schema.json "open original schema") |

## server Type

`object` ([Server configuration](configuration-properties-server-configuration.md))

# server Properties

| Property            | Type     | Required | Nullable       | Defined by                                                                                                                                  |
| :------------------ | :------- | :------- | :------------- | :------------------------------------------------------------------------------------------------------------------------------------------ |
| [address](#address) | `string` | Required | cannot be null | [Kiwi configuration](configuration-properties-server-configuration-properties-address.md "undefined#/properties/server/properties/address") |

## address

Server address and port

`address`

*   is required

*   Type: `string`

*   cannot be null

*   defined in: [Kiwi configuration](configuration-properties-server-configuration-properties-address.md "undefined#/properties/server/properties/address")

### address Type

`string`

### address Examples

```json
"127.0.0.1:8000"
```
