---
sidebar_label: boolean
---

# Boolean

`boolean` can be used to validate if the input is exactly `true` or `false`. This does not pass truthy values.

For example, if the `pull_request.draft` value is `false`:

```json
{
  "action": "opened",
  "number": 2,
  "pull_request": {
    "draft": false
  }
}
```

This passes the validation, because the value of `draft` exactly matches `false`:

```yaml
- do: payload
  pull_request:
    draft:
      boolean:
        match: false
        message: 'Custom message...' # this is optional, a default message is used when not specified.
```

| Param | Description | Required | Default Message |
|----|----|----|----|
| match | Bool value to check for | Yes |  |
| message | Message to show if the validation fails | No | The \[INPUT NAME\] must be \[match\] |

## Supported filters

```yaml
'payload'
```
