# Kafka source configuration Schema

```txt
undefined#/properties/sources/items/oneOf/1
```



| Abstract            | Extensible | Status         | Identifiable | Custom Properties | Additional Properties | Access Restrictions | Defined In                                                                      |
| :------------------ | :--------- | :------------- | :----------- | :---------------- | :-------------------- | :------------------ | :------------------------------------------------------------------------------ |
| Can be instantiated | No         | Unknown status | No           | Forbidden         | Allowed               | none                | [configuration.schema.json\*](configuration.schema.json "open original schema") |

## 1 Type

`object` ([Kafka source configuration](configuration-properties-source-configuration-items-oneof-kafka-source-configuration.md))

# 1 Properties

| Property        | Type          | Required | Nullable       | Defined by                                                                                                                                                                                    |
| :-------------- | :------------ | :------- | :------------- | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [type](#type)   | Not specified | Required | cannot be null | [Kiwi configuration](configuration-properties-source-configuration-items-oneof-kafka-source-configuration-properties-type.md "undefined#/properties/sources/items/oneOf/1/properties/type")   |
| [topic](#topic) | `string`      | Required | cannot be null | [Kiwi configuration](configuration-properties-source-configuration-items-oneof-kafka-source-configuration-properties-topic.md "undefined#/properties/sources/items/oneOf/1/properties/topic") |

## type



`type`

*   is required

*   Type: unknown

*   cannot be null

*   defined in: [Kiwi configuration](configuration-properties-source-configuration-items-oneof-kafka-source-configuration-properties-type.md "undefined#/properties/sources/items/oneOf/1/properties/type")

### type Type

unknown

### type Constraints

**constant**: the value of this property must be equal to:

```json
"kafka"
```

## topic

Kafka topic

`topic`

*   is required

*   Type: `string`

*   cannot be null

*   defined in: [Kiwi configuration](configuration-properties-source-configuration-items-oneof-kafka-source-configuration-properties-topic.md "undefined#/properties/sources/items/oneOf/1/properties/topic")

### topic Type

`string`
