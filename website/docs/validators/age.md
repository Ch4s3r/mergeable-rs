---
sidebar_label: age
---

# Age

Validates the age of a pull request: the time since it was created (`created_at`) or last updated (`updated_at`).

```yaml
- do: age # validate based on the age of the PR
  created_at:
    days: 1
    message: 'PR needs to be at least 1 day old in order to merge' # optional, custom message to display if the validation fails
  updated_at:
    days: 1
    message: 'PR needs to be update free for 1 day before merging' # optional, custom message to display if the validation fails
```

## Supported events

```yaml
'pull_request.*', 'pull_request_review.*', 'issue_comment.*'
```
