# Subscriber configuration Schema

```txt
undefined#/properties/subscriber
```

Subscriber configuration

| Abstract            | Extensible | Status         | Identifiable | Custom Properties | Additional Properties | Access Restrictions | Defined In                                                                      |
| :------------------ | :--------- | :------------- | :----------- | :---------------- | :-------------------- | :------------------ | :------------------------------------------------------------------------------ |
| Can be instantiated | No         | Unknown status | No           | Forbidden         | Allowed               | none                | [configuration.schema.json\*](configuration.schema.json "open original schema") |

## subscriber Type

`object` ([Subscriber configuration](configuration-properties-subscriber-configuration.md))

# subscriber Properties

| Property                                        | Type      | Required | Nullable       | Defined by                                                                                                                                                                    |
| :---------------------------------------------- | :-------- | :------- | :------------- | :---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [buffer\_capacity](#buffer_capacity)            | `integer` | Optional | cannot be null | [Kiwi configuration](configuration-properties-subscriber-configuration-properties-buffer_capacity.md "undefined#/properties/subscriber/properties/buffer_capacity")           |
| [lag\_notice\_threshold](#lag_notice_threshold) | `integer` | Optional | cannot be null | [Kiwi configuration](configuration-properties-subscriber-configuration-properties-lag_notice_threshold.md "undefined#/properties/subscriber/properties/lag_notice_threshold") |

## buffer\_capacity

Buffer capacity for pull-based subscriptions

`buffer_capacity`

*   is optional

*   Type: `integer`

*   cannot be null

*   defined in: [Kiwi configuration](configuration-properties-subscriber-configuration-properties-buffer_capacity.md "undefined#/properties/subscriber/properties/buffer_capacity")

### buffer\_capacity Type

`integer`

### buffer\_capacity Constraints

**minimum**: the value of this number must greater than or equal to: `1`

## lag\_notice\_threshold

The maximum number of events that a pull-based subscriber is permitted to lag behind the source before kiwi emits a lag notice

`lag_notice_threshold`

*   is optional

*   Type: `integer`

*   cannot be null

*   defined in: [Kiwi configuration](configuration-properties-subscriber-configuration-properties-lag_notice_threshold.md "undefined#/properties/subscriber/properties/lag_notice_threshold")

### lag\_notice\_threshold Type

`integer`

### lag\_notice\_threshold Constraints

**minimum**: the value of this number must greater than or equal to: `1`
