---
sidebar_label: no_empty
---

# NoEmpty

`no_empty` validates that the input is not empty.

```yaml
- do: description
  no_empty:
     enabled: false # Cannot be empty when true.
     message: 'Custom message...' # this is optional, a default message is used when not specified.
```

| Param | Description | Required | Default Message |
|----|----|----|----|
| enabled | Bool value to enable/disable the option | Yes |  |
| message | Message to show if the validation fails | No | The \[INPUT NAME\] can't be empty |

## Supported validators

```yaml
'changeset', 'description', 'label', 'milestone', 'title'
```
