---
sidebar_label: required
---

# Required

`required` validates that the given reviewers have approved the pull request.

```yaml
- do: approvals
  required:
    reviewers: [ user1, user2 ] # list of github usernames required to review
    owners: true # Optional boolean. When true, the file .github/CODEOWNERS is read and owners made required reviewers
    assignees: true # Optional boolean. When true, PR assignees are made required reviewers.
    requested_reviewers: true # Optional boolean. When true, all the requested reviewer's approval is required
    message: 'Custom message...'
```

| Param | Description | Required | Default Message |
|----|----|----|----|
| reviewers | An array value for github users/teams to be required to do the validation | No | \[\] |
| owners | The file .github/CODEOWNERS is read and owners made required reviewers | No | \[\] |
| assignees | PR assignees are made required reviewers. | No | \[\] |
| requested_reviewers | All the requested reviewer's approval is required | No | \[\] |
| message | Message to show if the validation fails | No | |

## Supported validators

```yaml
'approvals'
```
