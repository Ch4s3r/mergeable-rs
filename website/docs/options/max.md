---
sidebar_label: max
---

# Max

`max` validates that the number of items in the input (assignees, approvals, files, labels) is not more than the given count.

```yaml
- do: assignee
  max:
    count: 2 # There should not be more than 2 assignees
    message: 'test string' # this is optional
```

| Param | Description | Required | Default Message |
|----|----|----|----|
| count | number to validate input's length | Yes |  |
| message | Message to show if the validation fails | No | \[INPUT NAME\] count is more than \[COUNT\] |

## Supported validators

```yaml
'approvals', 'assignee', 'changeset', 'label'
```
