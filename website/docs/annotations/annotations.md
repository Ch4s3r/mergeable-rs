---
title: Annotations
---

# Annotations

Annotations allow the use of dynamic values in your recipes.

```text
@author : replaced with the login of the creator of the issue/PR
@sender : replaced with the login of the initiator of the event
@bot : replaced with the name of the Mergeable bot (APP_NAME followed by [bot], e.g. Mergeable[bot])
@repository : replaced with the full name (owner/repo) of the repository of the issue/PR
@action : replaced with the action of the event (e.g. opened)
```

To bypass an annotation, use the `\` prefix (i.e. `\@author` is replaced with the literal `@author`).

:::note

Only the first occurrence of each annotation in a text is replaced, as in the original implementation.

:::

Annotations are supported in:

- the actions [assign](../actions/assign.md) (`assignees`), [comment](../actions/comment.md) (`payload.body`) and
  [checks](../actions/checks.md) (`payload`),
- the lists of [one_of and none_of](../options/one-of-none-of.md) (validators and filters `author`, filter `payload`,
  `comment_author` of the `lastComment` validator).

For values from the pull request, the issue or the validation result use [templates](./templates.md).
