---
sidebar_label: begins_with
---

# BeginsWith

`begins_with` validates that the input begins with the given string (or one of the given strings).

```yaml
- do: milestone
  begins_with:
     match: 'A String' # or an array of strings
     message: 'Some message...'
```

| Param | Description | Required | Default Message |
|----|----|----|----|
| match | string or array of strings the input must begin with | Yes |  |
| message | Message to show if the validation fails | No | \[INPUT NAME\] must begin with \[MATCH\] |

## Supported validators

```yaml
'changeset', 'contents', 'description', 'label', 'milestone', 'title'
```
