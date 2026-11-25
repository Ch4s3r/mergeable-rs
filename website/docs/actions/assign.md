---
sidebar_label: assign
---

# Assign

You can assign specific people to a pull request or issue.

```yaml
- do: assign
  assignees: [ 'shine2lay', 'jusx', '@author' ] # only array accepted, use @author for PR/Issue author, use @sender for event initiator, use @bot for the Mergeable bot
```

## Supported events

```yaml
'pull_request.*', 'issues.*', 'issue_comment.*'
```
