---
sidebar_label: 'one_of / none_of'
---

# OneOf and NoneOf

`one_of` validates that the input is one of the listed values, `none_of` that it is none of them. The comparison is
case-insensitive. A list item can be:

- a login or any other plain string,
- an [annotation](../annotations/annotations.md) such as `@author`, `@sender` or `@bot`, replaced before the comparison,
- a team in the form `@org/team-slug`, replaced by the logins of the team members.

```yaml
- do: author
  one_of: ['user-1', '@org/team-slug'] # the author is user-1 or a member of the team
  none_of: ['user-2', '@bot'] # the author is neither user-2 nor the Mergeable bot
```

In the [payload](../filters/payload.md) filter the options apply to any field:

```yaml
- do: payload
  sender:
    login:
      one_of: ['@org/team-slug']
      none_of: ['banned_user', '@author', '@bot']
```

The [lastComment](../validators/last-comment.md) validator uses them in `comment_author` to select the comments
that are considered.

| Param | Description | Required |
|----|----|----|
| one_of | array of values; the input must be one of them | Yes, for one_of |
| none_of | array of values; the input must be none of them | Yes, for none_of |

## Supported validators and filters

```yaml
'author' (validator and filter), 'payload' (filter), 'lastComment' (comment_author)
```
