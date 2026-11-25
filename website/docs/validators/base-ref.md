---
sidebar_label: baseRef
---

# BaseRef

Validates the base branch (the branch the pull request merges into).

```yaml
- do: baseRef
  must_include:
     regex: 'master|feature-branch1'
     regex_flag: 'none' # Optional. Specify the flag for Regex. default is 'i', to disable default use 'none'
     message: 'Custom message...'
  must_exclude:
     regex: 'feature-branch2'
     regex_flag: 'none' # Optional. Specify the flag for Regex. default is 'i', to disable default use 'none'
     message: 'Custom message...'
  mediaType:  # Optional. Required by status.* events to enable the groot preview on some Github Enterprise servers
     previews: 'array'
```

Simple example:

```yaml
- do: baseRef
  must_exclude:
    regex: 'master'
    message: 'Merging into repo:master is forbidden'
```

Example with the groot preview enabled (for `status.*` events on some older GitHub Enterprise servers):

```yaml
- do: baseRef
  must_include:
    regex: 'master|main'
    message: 'Auto-merging is only enabled for default branch'
  mediaType:
    previews:
      - groot
```

## Supported events

```yaml
'pull_request.*', 'pull_request_review.*', 'check_suite.*', 'status.*'
```
