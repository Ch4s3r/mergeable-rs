---
sidebar_label: must_exclude
---

# MustExclude

`must_exclude` validates that the input does not match the given regular expression.

```yaml
- do: headRef
  must_exclude:
    regex: '^(feature|hotfix)\/.+$'
    message: |
        Your pull request doesn't adhere to the branch naming convention described <a href="some link">there</a>!
```

You can also use an array of regex matchers. If any of them match, the validation will fail.

```yaml
- do: headRef
  must_exclude:
    regex:
      - "^bug"
      - "^breaking"
      - "^test"
    message: |
        Your pull request doesn't adhere to the branch naming convention described <a href="some link">there</a>!
```

| Param | Description | Required | Default Message |
|----|----|----|----|
| regex | regular expression, or an array of them | Yes |  |
| message | Message to show if the validation fails | No | \[INPUT NAME\] does not exclude \[REGEX\] |
| regex_flag | Regex flag to be used with regex param to validate inputs | No | i |

`must_exclude` is also supported by the filters `author`, `baseRef`, `payload` and `repository` (`name`, `topics`).

## Supported validators

```yaml
'author', 'baseRef', 'headRef', 'changeset', 'contents', 'description', 'label', 'lastComment', 'milestone', 'title'
```
