---
sidebar_label: ends_with
---

# EndsWith

`ends_with` validates that the input ends with the given string (or one of the given strings).

```yaml
- do: milestone
  ends_with:
     match: 'A String' # or an array of strings
     message: 'Some message...'
```

| Param | Description | Required | Default Message |
|----|----|----|----|
| match | string or array of strings the input must end with | Yes |  |
| message | Message to show if the validation fails | No | \[INPUT NAME\] must end with \[MATCH\] |

## Supported validators

```yaml
'changeset', 'contents', 'description', 'label', 'milestone', 'title'
```
