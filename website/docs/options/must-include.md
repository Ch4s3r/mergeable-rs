---
sidebar_label: must_include
---

# MustInclude

`must_include` validates that the input matches the given regular expression. For a list input (labels, files) one matching item is enough.

```yaml
- do: headRef
  must_include:
    regex: '^(feature|hotfix)\/.+$'
    message: |
        Your pull request doesn't adhere to the branch naming convention described <a href="some link">there</a>!
```

You can also use an array of regex matchers. If any of them match, the validation will pass.

```yaml
- do: headRef
  must_include:
    regex:
      - "^feature"
      - "^hotfix"
      - "^fix"
    message: |
        Your pull request doesn't adhere to the branch naming convention described <a href="some link">there</a>!
```

| Param | Description | Required | Default Message |
|----|----|----|----|
| regex | regular expression, or an array of them | Yes |  |
| message | Message to show if the validation fails | No | \[INPUT NAME\] does not include \[REGEX\] |
| regex_flag | Regex flag to be used with regex param to validate inputs | No | i |

`must_include` is also supported by the filters `author`, `baseRef`, `payload` and `repository` (`name`, `topics`).

## Supported validators

```yaml
'author', 'baseRef', 'headRef', 'changeset', 'contents', 'description', 'label', 'lastComment', 'milestone', 'project', 'title'
```
