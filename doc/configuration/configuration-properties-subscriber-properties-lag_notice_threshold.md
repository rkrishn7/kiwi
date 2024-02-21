# Untitled integer in Kiwi Configuration Schema

```txt
undefined#/properties/subscriber/properties/lag_notice_threshold
```

The maximum number of events that a pull-based subscriber is permitted to lag behind the source before kiwi emits a lag notice

| Abstract            | Extensible | Status         | Identifiable            | Custom Properties | Additional Properties | Access Restrictions | Defined In                                                                      |
| :------------------ | :--------- | :------------- | :---------------------- | :---------------- | :-------------------- | :------------------ | :------------------------------------------------------------------------------ |
| Can be instantiated | No         | Unknown status | Unknown identifiability | Forbidden         | Allowed               | none                | [configuration.schema.json\*](configuration.schema.json "open original schema") |

## lag\_notice\_threshold Type

`integer`

## lag\_notice\_threshold Constraints

**minimum**: the value of this number must greater than or equal to: `1`
