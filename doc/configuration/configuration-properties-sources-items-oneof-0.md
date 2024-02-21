# Untitled object in Kiwi Configuration Schema

```txt
undefined#/properties/sources/items/oneOf/0
```



| Abstract            | Extensible | Status         | Identifiable | Custom Properties | Additional Properties | Access Restrictions | Defined In                                                                      |
| :------------------ | :--------- | :------------- | :----------- | :---------------- | :-------------------- | :------------------ | :------------------------------------------------------------------------------ |
| Can be instantiated | No         | Unknown status | No           | Forbidden         | Allowed               | none                | [configuration.schema.json\*](configuration.schema.json "open original schema") |

## 0 Type

`object` ([Details](configuration-properties-sources-items-oneof-0.md))

# 0 Properties

| Property                     | Type          | Required | Nullable       | Defined by                                                                                                                                                          |
| :--------------------------- | :------------ | :------- | :------------- | :------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [type](#type)                | Not specified | Required | cannot be null | [Kiwi Configuration](configuration-properties-sources-items-oneof-0-properties-type.md "undefined#/properties/sources/items/oneOf/0/properties/type")               |
| [id](#id)                    | `string`      | Required | cannot be null | [Kiwi Configuration](configuration-properties-sources-items-oneof-0-properties-id.md "undefined#/properties/sources/items/oneOf/0/properties/id")                   |
| [interval\_ms](#interval_ms) | `integer`     | Required | cannot be null | [Kiwi Configuration](configuration-properties-sources-items-oneof-0-properties-interval_ms.md "undefined#/properties/sources/items/oneOf/0/properties/interval_ms") |
| [lazy](#lazy)                | `boolean`     | Optional | cannot be null | [Kiwi Configuration](configuration-properties-sources-items-oneof-0-properties-lazy.md "undefined#/properties/sources/items/oneOf/0/properties/lazy")               |
| [min](#min)                  | `number`      | Required | cannot be null | [Kiwi Configuration](configuration-properties-sources-items-oneof-0-properties-min.md "undefined#/properties/sources/items/oneOf/0/properties/min")                 |
| [max](#max)                  | `number`      | Optional | cannot be null | [Kiwi Configuration](configuration-properties-sources-items-oneof-0-properties-max.md "undefined#/properties/sources/items/oneOf/0/properties/max")                 |

## type



`type`

*   is required

*   Type: unknown

*   cannot be null

*   defined in: [Kiwi Configuration](configuration-properties-sources-items-oneof-0-properties-type.md "undefined#/properties/sources/items/oneOf/0/properties/type")

### type Type

unknown

### type Constraints

**constant**: the value of this property must be equal to:

```json
"counter"
```

## id

Unique source identifier

`id`

*   is required

*   Type: `string`

*   cannot be null

*   defined in: [Kiwi Configuration](configuration-properties-sources-items-oneof-0-properties-id.md "undefined#/properties/sources/items/oneOf/0/properties/id")

### id Type

`string`

## interval\_ms

Interval (in milliseconds) at which the counter emits events

`interval_ms`

*   is required

*   Type: `integer`

*   cannot be null

*   defined in: [Kiwi Configuration](configuration-properties-sources-items-oneof-0-properties-interval_ms.md "undefined#/properties/sources/items/oneOf/0/properties/interval_ms")

### interval\_ms Type

`integer`

### interval\_ms Constraints

**minimum**: the value of this number must greater than or equal to: `1`

## lazy

Start emitting events upon the first subscription

`lazy`

*   is optional

*   Type: `boolean`

*   cannot be null

*   defined in: [Kiwi Configuration](configuration-properties-sources-items-oneof-0-properties-lazy.md "undefined#/properties/sources/items/oneOf/0/properties/lazy")

### lazy Type

`boolean`

## min

Minimum value

`min`

*   is required

*   Type: `number`

*   cannot be null

*   defined in: [Kiwi Configuration](configuration-properties-sources-items-oneof-0-properties-min.md "undefined#/properties/sources/items/oneOf/0/properties/min")

### min Type

`number`

## max

Maximum value

`max`

*   is optional

*   Type: `number`

*   cannot be null

*   defined in: [Kiwi Configuration](configuration-properties-sources-items-oneof-0-properties-max.md "undefined#/properties/sources/items/oneOf/0/properties/max")

### max Type

`number`
