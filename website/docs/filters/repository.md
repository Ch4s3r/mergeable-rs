---
sidebar_label: repository
---

# Repository

Runs the validators only if the repository matches: its visibility, name or topics.

```yaml
- do: repository
  visibility: 'public' # Can be public or private
  name:
    must_include:
      regex: 'my-repo-name'
    must_exclude:
      regex: 'other-repo-name'
  topics:
    must_include:
      regex: 'my-topic'
      message: 'Custom message...'
    must_exclude:
      regex: 'other-topic'
      message: 'Custom message...'
    # all of the message sub-option is optional
```

You can use `and` and `or` options to create more complex filters

```yaml
- do: repository
  topics:
    and:
      - must_include:
          regex: 'topic-1'
          message: 'Custom message...'
      - must_include:
          regex: 'topic-2'
          message: 'Custom message...'
    or:
      - must_include:
          regex: 'topic-3'
          message: 'Custom message...'
      - must_include:
          regex: 'topic-4'
          message: 'Custom message...'
```

You can also nest `and` and `or` options

```yaml
- do: repository
  topics:
    and:
      - or:
          - must_include:
              regex: 'topic-1'
              message: 'Custom message...'
          - must_include:
              regex: 'topic-2'
              message: 'Custom message...'
      - must_include:
          regex: 'topic-3'
          message: 'Custom message...'
```

## Supported events

```yaml
'pull_request.*', 'pull_request_review.*'
```
