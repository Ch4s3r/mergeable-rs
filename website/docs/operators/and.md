---
sidebar_label: and
---

# And

`and`, `or` and `not` can be used to create more complex validation and filter checks. `and` passes when all of its validators or filters pass.

```yaml
filter:
  - do: and
    filter:
      - do: author
        must_include:
          regex: 'user-1'
      - do: repository
        visibility: public
validate:
  - do: and
    validate:
      - do: title
        begins_with:
          match: '[WIP]'
      - do: label
        must_include:
          regex: 'Ready to Merge'
```

You can also nest `and`, `or` and `not`:

```yaml
filter:
  - do: and
    filter:
      - do: or
        filter:
          - do: author
            must_include:
              regex: 'user-1'
          - do: author
            must_include:
              regex: 'user-2'
      - do: repository
        visibility: public
validate:
  - do: and
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
