---
sidebar_label: label
---

# Label

Validates the labels of the pull request or issue.

```yaml
- do: label
  no_empty:
     enabled: false # Cannot be empty when true.
     message: 'Custom message...'
  must_include:
     regex: 'type|chore|wont'
     regex_flag: 'none' # Optional. Specify the flag for Regex. default is 'i', to disable default use 'none'
     message: 'Custom message...'
  must_exclude:
     regex: 'DO NOT MERGE'
     regex_flag: 'none' # Optional. Specify the flag for Regex. default is 'i', to disable default use 'none'
     message: 'Custom message...'
  begins_with:
     match: 'A String' # or array of strings
     message: 'Some message...'
  ends_with:
     match: 'A String' # or array of strings
     message: 'Come message...'
  jira:
    regex: '[A-Z][A-Z0-9]+-\d+'
    regex_flag: none
    message: 'The Jira ticket does not exist'
  min:
    count: 1 # Optional. Minimum number of labels.
  max:
    count: 3 # Optional. Maximum number of labels.
  # all of the message sub-option is optional
```

You can use `and` and `or` options to create more complex validations

```yaml
- do: label
  and:
    - must_include:
        regex: 'big|medium|small'
        message: 'Custom message...'
    - must_include:
        regex: 'type|chore|wont'
        message: 'Custom message...'
  or:
    - must_include:
        regex: 'Ready to merge'
        message: 'Custom message...'
    - must_include:
        regex: 'DO NOT MERGE'
        message: 'Custom message...'
```

You can also nest `and` and `or` options

```yaml
- do: label
  and:
    - or:
      - must_include:
          regex: 'feat|fix|chore'
          message: 'Custom message...'
      - must_include:
          regex: 'major|minor|patch'
          message: 'Custom message...'
    - must_include:
        regex: 'Ready to merge'
        message: 'Custom message...'
```

## Supported events

```yaml
'pull_request.*', 'pull_request_review.*', 'issues.*', 'issue_comment.*'
```
