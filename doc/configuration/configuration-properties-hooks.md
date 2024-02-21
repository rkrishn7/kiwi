# Untitled object in Kiwi Configuration Schema

```txt
undefined#/properties/hooks
```

WebAssembly hooks

| Abstract            | Extensible | Status         | Identifiable | Custom Properties | Additional Properties | Access Restrictions | Defined In                                                                      |
| :------------------ | :--------- | :------------- | :----------- | :---------------- | :-------------------- | :------------------ | :------------------------------------------------------------------------------ |
| Can be instantiated | No         | Unknown status | No           | Forbidden         | Allowed               | none                | [configuration.schema.json\*](configuration.schema.json "open original schema") |

## hooks Type

`object` ([Details](configuration-properties-hooks.md))

# hooks Properties

| Property                      | Type     | Required | Nullable       | Defined by                                                                                                                            |
| :---------------------------- | :------- | :------- | :------------- | :------------------------------------------------------------------------------------------------------------------------------------ |
| [authenticate](#authenticate) | `string` | Optional | cannot be null | [Kiwi Configuration](configuration-properties-hooks-properties-authenticate.md "undefined#/properties/hooks/properties/authenticate") |
| [intercept](#intercept)       | `string` | Optional | cannot be null | [Kiwi Configuration](configuration-properties-hooks-properties-intercept.md "undefined#/properties/hooks/properties/intercept")       |

## authenticate

Path to an authenticate WASM module

`authenticate`

*   is optional

*   Type: `string`

*   cannot be null

*   defined in: [Kiwi Configuration](configuration-properties-hooks-properties-authenticate.md "undefined#/properties/hooks/properties/authenticate")

### authenticate Type

`string`

## intercept

Path to an intercept WASM module

`intercept`

*   is optional

*   Type: `string`

*   cannot be null

*   defined in: [Kiwi Configuration](configuration-properties-hooks-properties-intercept.md "undefined#/properties/hooks/properties/intercept")

### intercept Type

`string`
