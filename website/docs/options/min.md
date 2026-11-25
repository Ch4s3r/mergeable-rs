---
sidebar_label: min
---

# Min

`min` validates that the number of items in the input (assignees, approvals, files, labels) is not less than the given count.

```yaml
- do: assignee
  min:
    count: 2 # There should be at least 2 assignees
    message: 'test string' # this is optional
```

| Param | Description | Required | Default Message |
|----|----|----|----|
| count | number to validate input's length | Yes |  |
| message | Message to show if the validation fails | No | \[INPUT NAME\] count is less than \[COUNT\] |

## Supported validators

```yaml
'approvals', 'assignee', 'changeset', 'label'
```
