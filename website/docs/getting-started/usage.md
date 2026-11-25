---
title: Usage
---

# Usage

1. Run your own instance of mergeable-rs and register it as a GitHub App, see [GitHub App setup](../deployment/github-app.md)
   and [Container image](../deployment/container-image.md). mergeable-rs has no public hosted app.
2. Install the GitHub App on the repositories or the organisation you want to automate.
3. Create your recipe(s) using the [configuration reference](../configuration/configuration.md) or start from one of the
   [recipes](../recipes.md).
4. Commit and push the recipes to your repository at `.github/mergeable.yml`.
5. (Optional) Create a default configuration for an organisation by creating a repository called `.github` and adding
   your file there. See [organisation-wide defaults](../configuration/configuration.md#organisation-wide-defaults).

A minimal configuration that requires a description on every pull request:

```yaml
version: 2
mergeable:
  - when: pull_request.*
    validate:
      - do: description
        no_empty:
          enabled: true
```

Open or edit a pull request: mergeable creates a check run named `Mergeable` (`Mergeable: <name>` when the recipe has a
`name`) that passes or fails with the details of every validator.

:::note

The configuration format is the same as the one of the original mergeable, so a `.github/mergeable.yml` written for the
original works unchanged.

:::
