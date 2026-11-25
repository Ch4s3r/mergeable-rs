---
sidebar_label: not
---

# Not

`and`, `or` and `not` can be used to create more complex validation and filter checks. `not` inverts the result of
exactly one validator or filter: it passes when its child fails and fails when its child passes.

```yaml
filter:
  - do: not
    filter:
      - do: author
        must_include:
          regex: 'user-1'
validate:
  - do: not
    validate:
      - do: title
        begins_with:
          match: '[WIP]'
```

:::warning

The `filter` or `validate` list of `not` must contain exactly one element. With more elements the result is an error
(`Failed to validate because the 'validate' option does not have exactly one element`). To negate several checks, wrap
them in `and` or `or` first.

:::

You can also nest `and`, `or` and `not`:

```yaml
filter:
  - do: not
    filter:
      - do: or
        filter:
          - do: author
            must_include:
              regex: 'user-1'
          - do: author
            must_include:
              regex: 'user-2'
validate:
  - do: and
    validate:
      - do: not
        validate:
          - do: or
            validate:
              - do: title
                begins_with:
                  match: 'feat:'
              - do: label
                must_include:
                  regex: 'feature'
      - do: label
        must_include:
          regex: 'Ready to Merge'
```
