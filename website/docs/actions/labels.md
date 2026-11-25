---
sidebar_label: labels
---

# Labels

You can add, replace and delete labels with one action.

:::warning

Using multiple `do: labels` in the same operation is not idempotent. Use only one `labels` action per block for accurate labeling.

:::

You can add new labels, preserving existing ones:

```yaml
- do: labels
  add: 'Ready for Review'
```

You can delete existing labels:

```yaml
- do: labels
  delete: [ 'Ready for Review', 'Triage' ]
```

You can replace all current labels with new ones:

```yaml
- do: labels
  replace: [ 'Triage', 'Needs Deploy' ]
```

You can also use any combination of these options. They can be listed in any order, but the action is always evaluated in the order of `replace` → `add` → `delete`.

```yaml
- do: labels
  replace: [ 'New Task', 'Not Useful' ]
  add: [ 'Work in Progress', 'Needs Deploy' ]
  delete: 'Not Useful'

# result: [ 'New Task', 'Work in Progress', 'Needs Deploy' ]
```

## `labels` and `mode`

:::warning

Using `labels` with `mode` is deprecated and will be removed in v3. Use the `add`, `replace`, and `delete` options above for labeling.

:::

You can also add a set of labels to an item:

```yaml
- do: labels
  # if label doesn't exist, it'll be created
  labels: [ 'Triage' ] # Only arrays are accepted
  mode: 'add' # Optional , default is 'add'. Other options : 'replace', 'delete'
```

You can also replace all of the labels on an item with a given set of labels:

```yaml
- do: labels
  # if label doesn't exist, it'll be created
  labels: [ 'Triage' ] # Only arrays are accepted
  mode: 'replace' # Replaces all of the labels with the above array of labels
```

You can also delete existing labels on an item and specify glob patterns when the mode is `delete`:

```yaml
- do: labels
  # if label doesn't exist, it'll be created
  labels: [ 'feature-*' ] # All labels beginning with 'feature-' will be removed
  mode: 'delete'
```

The glob patterns follow the [minimatch](https://github.com/isaacs/minimatch) semantics of the original implementation.

## Supported events

```yaml
'schedule.repository', 'pull_request.*', 'issues.*', 'issue_comment.*'
```
