# Hook configuration Schema

```txt
undefined#/properties/hooks
```

WebAssembly hooks

| Abstract            | Extensible | Status         | Identifiable | Custom Properties | Additional Properties | Access Restrictions | Defined In                                                                      |
| :------------------ | :--------- | :------------- | :----------- | :---------------- | :-------------------- | :------------------ | :------------------------------------------------------------------------------ |
| Can be instantiated | No         | Unknown status | No           | Forbidden         | Allowed               | none                | [configuration.schema.json\*](configuration.schema.json "open original schema") |

## hooks Type

`object` ([Hook configuration](configuration-properties-hook-configuration.md))

# hooks Properties

| Property                      | Type     | Required | Nullable       | Defined by                                                                                                                                         |
| :---------------------------- | :------- | :------- | :------------- | :------------------------------------------------------------------------------------------------------------------------------------------------- |
| [authenticate](#authenticate) | `string` | Optional | cannot be null | [Kiwi configuration](configuration-properties-hook-configuration-properties-authenticate.md "undefined#/properties/hooks/properties/authenticate") |
| [intercept](#intercept)       | `string` | Optional | cannot be null | [Kiwi configuration](configuration-properties-hook-configuration-properties-intercept.md "undefined#/properties/hooks/properties/intercept")       |

## authenticate

Path to an authenticate WASM module

`authenticate`

*   is optional

*   Type: `string`

*   cannot be null

*   defined in: [Kiwi configuration](configuration-properties-hook-configuration-properties-authenticate.md "undefined#/properties/hooks/properties/authenticate")

### authenticate Type

`string`

## intercept

Path to an intercept WASM module

`intercept`

*   is optional

*   Type: `string`

*   cannot be null

*   defined in: [Kiwi configuration](configuration-properties-hook-configuration-properties-intercept.md "undefined#/properties/hooks/properties/intercept")

### intercept Type

`string`
