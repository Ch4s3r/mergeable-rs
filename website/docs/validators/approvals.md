---
sidebar_label: approvals
---

# Approvals

```yaml
- do: approvals
  min:
    count: 2 # Number of minimum reviewers. In this case 2.
    message: 'Custom message...'
  max:
    count: 5 # Optional. Maximum number of approvals.
    message: 'Custom message...'
  required:
    reviewers: [ user1, user2 ] # list of github usernames required to review
    owners: true # Optional boolean. When true, the file .github/CODEOWNERS is read and owners made required reviewers
    assignees: true # Optional boolean. When true, PR assignees are made required reviewers.
    requested_reviewers: true # Optional boolean. When true, all the requested reviewer's approval is required
    message: 'Custom message...'
  block:
    changes_requested: true # If true, block all approvals when one of the reviewers gave 'changes_requested' review
    message: 'Custom message...'
  limit:
    teams: ['org/team_slug'] # when the option is present, only the approvals from the team members will count
    users: ['user1', 'user2'] # when the option is present, approvals from users in this list will count
    owners: true # Optional boolean. When true, the file .github/CODEOWNERS is read and only owners approval will count
  exclude:
    users: ['bot1', 'bot2'] # when the option is present, approvals from users in this list will NOT count
```

:::note

In the `limit` option, if more than one sub-option is present, the union of the results is used.

:::

:::note

If the `limit` option resolves to nobody, all approvals count as if `limit` were not set. For example, `limit.owners: true` in a repository without a CODEOWNERS file does not filter out any approval, the same way `required.owners` passes when the CODEOWNERS file is missing. The info output "Only approvals from following sources are counted" is still shown.

:::

:::note

If you receive the error `Resource not accessible by integration` for the owners file, the GitHub App lacks read permission for repository contents.

:::

:::note

The `owners` file supports teams as well, use the `@organization/team-slug` format.

:::

## Supported events

```yaml
'pull_request.*', 'pull_request_review.*'
```
