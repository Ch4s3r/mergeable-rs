---
title: Settings file
---

# Settings file

Besides the rules in `.github/mergeable.yml`, a few app settings can be set per organisation or per repository in
`.github/mergeable.settings.yml` (the file name can be changed with the `SETTINGS_PATH` environment variable).

Mergeable looks for the file in the organisation's `.github` repository first
(`<YOUR_ORG>/.github/.github/mergeable.settings.yml`) and then in the repository itself. The first file found is used.

```yaml
version: 1
mergeable:
  use_config_from_pull_request: true
  use_config_cache: false
  use_org_as_default_config: false
  config_path: mergeable.yml
```

| Setting | Meaning | Default |
|---|---|---|
| `use_config_from_pull_request` | Use the `.github/mergeable.yml` changed in a pull request from the same repository | `true` |
| `use_config_cache` | Cache the merged configuration per repository (see [Caching](../operations/caching.md)) | `USE_CONFIG_CACHE` |
| `use_org_as_default_config` | Merge the organisation `.github` configuration into every repository configuration | `USE_ORG_AS_DEFAULT_CONFIG` |
| `config_path` | Configuration file name below `.github/` | `CONFIG_PATH`, else `mergeable.yml` |

Settings missing from the file take the default from the environment variable named in the table. Only `version: 1`
is supported. A file with another version or invalid YAML is ignored with a warning in the log; then none of the
environment defaults apply either: the configuration is read from `mergeable.yml`, uncached, without the organisation
defaults, and from the pull request.

With `USE_SETTINGS_CACHE=true` the settings file is cached per repository.
