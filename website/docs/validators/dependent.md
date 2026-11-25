---
sidebar_label: dependent
---

# Dependent

Validates that the files specified are all part of a pull request (added or modified).

```yaml
- do: dependent
  files: ['package.json', 'yarn.lock'] # list of files that are dependent on one another and must all be part of the changes in a PR.
  message: 'Custom message...' # this is optional, a default message is used when not specified.
```

Alternatively, to validate dependent files only when a specific file is part of the pull request, use the changed option:

```yaml
- do: dependent
  changed:
    file: package.json # also supports glob expressions
    required: ['package-lock.json', 'yarn.lock'] # alias: `files` for backward compatibility
  message: 'Custom message...' # this is optional, a default message is used when not specified.
```

The above validates that both `package-lock.json` and `yarn.lock` are part of the modified or added files if and only if `package.json` is part of the pull request.

## Supported events

```yaml
'pull_request.*', 'pull_request_review.*'
```
