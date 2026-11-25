---
sidebar_label: baseRef
---

# BaseRef

Runs the validators only if the base branch of the pull request matches.

```yaml
- do: baseRef
  must_include:
      regex: 'some-ref'
      message: 'Custom message...'
  # all of the message sub-option is optional
```

You can use `and` and `or` options to create more complex filters

```yaml
- do: baseRef
  and:
    - must_exclude:
        regex: 'some-other-ref'
        message: 'Custom message...'
  or:
    - must_include:
        regex: 'some-ref'
        message: 'Custom message...'
    - must_include:
        regex: 'some-other-ref'
        message: 'Custom message...'
```

You can also nest `and` and `or` options

```yaml
- do: baseRef
  and:
    - or:
        - must_include:
            regex: 'some-ref'
            message: 'Custom message...'
        - must_include:
            regex: 'some-other-ref'
            message: 'Custom message...'
    - must_exclude:
        regex: 'yet-another-ref'
        message: 'Custom message...'
```

## Supported events

```yaml
'pull_request.*', 'pull_request_review.*'
```
