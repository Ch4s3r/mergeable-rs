---
sidebar_label: size
---

# Size

`size` validates that the size of changes in the pull request conform to a specified limit. We can pass in three options: `total`, `additions` or `deletions`. Each of them takes a `count` and a `message`.

```yaml
- do: size
  lines:
    total:
      count: 500
      message: Change is very large. Should be under 500 lines of additions and deletions.
    additions:
      count: 250
      message: Change is very large. Should be under 250 lines of additions
    deletions:
      count: 250
      message: Change is very large. Should be under 250 lines of deletions.
    ignore_comments: false # if true, comments will not be counted toward the lines count
```

:::warning

`ignore_comments` only works for files with the `.js` and `.py` extensions.

:::

`max` is an alias for `total`, so the configuration below is still valid.

```yaml
- do: size
  lines:
    max:
      count: 500
      message: Change is very large. Should be under 500 lines of additions and deletions.
```

It also supports an `ignore` or `match` setting to allow excluding or including certain files from the total size (e.g. for ignoring automatically generated files that increase the size a lot).

This option supports glob patterns, so you can provide either the path to a specific file or ignore whole patterns:

```yaml
- do: size
  match: ['src']
  ignore: ['package-lock.json', 'src/tests/__snapshots__/**', 'docs/*.md']
  lines:
    total:
      count: 500
      message: Change is very large. Should be under 500 lines of additions and deletions
```

In case both `match` and `ignore` are specified, the `match` filters are applied first.

The glob patterns follow the [minimatch](https://github.com/isaacs/minimatch) semantics of the original implementation (reproduced by the built-in glob matcher of mergeable-rs).

The size validator currently excludes from the size count any files that were completely deleted in the pull request.

## Supported events

```yaml
'pull_request.*', 'pull_request_review.*'
```
