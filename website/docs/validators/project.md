---
sidebar_label: project
---

# Project

Validates the (classic) project the pull request or issue belongs to.

```yaml
- do: project
  must_include:
     regex: 'type|chore|wont'
     message: 'Custom message...'
```

:::note

When a closing keyword is used in the description of a pull request, the referenced issue is validated against the conditions as well.

:::

## Supported events

```yaml
'pull_request.*', 'pull_request_review.*', 'issues.*'
```
