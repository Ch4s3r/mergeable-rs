---
sidebar_label: author
---

# Author

Runs the validators only if the author (login) of the pull request matches. `one_of` and `none_of` accept user logins, annotations such as `@author` or `@bot` and teams (`@org/team-slug`), see [one_of and none_of](../options/one-of-none-of.md).

```yaml
- do: author
  must_include:
      regex: 'user-1'
      message: 'Custom include message...'  # optional
  must_exclude:
      regex: 'user-2'
      message: 'Custom exclude message...'  # optional
  team: 'org/team-slug'  # verify that the author is in the team
  one_of: ['user-1', '@org/team-slug']  # verify author for being one of the users or a team member
  none_of: ['user-2', '@bot']  # verify author for not being one of the users or the mergeable bot
```

You can use `and` and `or` options to create more complex filters

```yaml
- do: author
  and:
    - must_exclude:
        regex: 'bot-user-1'
        message: 'Custom message...'
  or:
    - must_include:
        regex: 'user-1'
        message: 'Custom message...'
    - must_include:
        regex: 'user-2'
        message: 'Custom message...'
```

You can also nest `and` and `or` options

```yaml
- do: author
  and:
    - or:
        - must_include:
            regex: 'user-1'
            message: 'Custom message...'
        - must_include:
            regex: 'user-2'
            message: 'Custom message...'
    - must_exclude:
        regex: 'bot-user-1'
        message: 'Custom message...'
```

## Supported events

```yaml
'pull_request.*', 'pull_request_review.*'
```
