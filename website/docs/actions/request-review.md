---
sidebar_label: request_review
---

# Request Review

You can request specific reviews from specific reviewers, teams, or both for a pull request.

```yaml
- do: request_review
  reviewers: ['name1', 'name2']
  teams: ['developers'] # team names without organization
```

## Supported events

```yaml
'pull_request.*'
```
