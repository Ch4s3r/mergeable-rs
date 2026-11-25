---
title: Configuration
slug: /configuration
---

# Configuration

**Mergeable** is **highly** configurable.

First, you'll need to start by creating a `.github/mergeable.yml` file in your repository.

:::tip

Check out the [recipes](../recipes.md) page for examples and the most commonly used settings.

:::

Next, we'll go into how the configuration is structured.

## Basics

A Mergeable configuration consists of an array of independent recipes where each recipe has the following properties:

| Property | Meaning |
|---|---|
| `when` | the webhook event(s) in which to process the validation, e.g. `pull_request.*` or `issues.opened, issues.edited` |
| `name` | a friendly name which appears on the pull request for the associated check (optional) |
| `filter` | a series of optional [filters](/filters); the validators only run if all filters pass |
| `validate` | a series of [validators](/validators) to be checked |
| `pass` | a series of [actions](/actions) to execute if the validation suite returned `pass` |
| `fail` | a series of actions to execute if the validation suite returned `fail` |
| `error` | a series of actions to execute if the validation suite returned `error` |

Each recipe appears as a separate check in the pull request.

Here is a full example of how a recipe looks (the `<...>` parts are placeholders):

```yaml
version: 2
mergeable:
  - when: <event>, <event> # can be one or more
    name: check name A
    filter:
      # list of filters (optional). Specify one or more.
      - do: <filter>
        <option>: # name of an option supported by the filter.
          <sub-option>: <value> # an option will have one or more sub-options.
    validate:
      # list of validators. Specify one or more.
      - do: <validator>
        <option>: # name of an option supported by the validator.
          <sub-option>: <value> # an option will have one or more sub-options.
    pass: # list of actions to be executed if all validation passes. Specify one or more. Omit this tag if no actions are needed.
      - do: <action>
    fail: # list of actions to be executed when at least one validation fails. Specify one or more. Omit this tag if no actions are needed.
      - do: <action>
    error: # list of actions to be executed when at least one validator throws an error. Specify one or more. Omit this tag if no actions are needed.
      - do: <action>

  - when: <event>, <event> # example for a second recipe
    name: check name B
    validate:
      - do: <validator>
    fail:
      - do: <action>
```

:::note

Some default actions are applied automatically based on the events specified: for pull request events a
[checks](../actions/checks.md) action reports the result as a check run unless the recipe only configures other actions.

:::

A configuration without `version` (the deprecated version 1 format) is still accepted; it is converted to version 2
before it is evaluated.

The events each validator, filter and action supports are listed on its page and summarised in
[Supported events](./events.md).

## Filters

Filters are checks that mergeable processes to determine whether the validators of a recipe run at all.

:::note

Each filter supports certain events only, so keep an eye out for them.

:::

Available filters: [author](../filters/author.md), [baseRef](../filters/base-ref.md), [payload](../filters/payload.md),
[repository](../filters/repository.md).

## Validators

Validators are checks that mergeable processes to determine whether an action should be executed.

:::note

Each validator supports certain events only, so keep an eye out for them.

:::

Available validators: [age](../validators/age.md), [approvals](../validators/approvals.md),
[assignee](../validators/assignee.md), [author](../validators/author.md), [baseRef](../validators/base-ref.md),
[changeset](../validators/changeset.md), [commit](../validators/commit.md), [contents](../validators/contents.md),
[dependent](../validators/dependent.md), [description](../validators/description.md),
[headRef](../validators/head-ref.md), [label](../validators/label.md), [lastComment](../validators/last-comment.md),
[milestone](../validators/milestone.md), [project](../validators/project.md), [size](../validators/size.md),
[stale](../validators/stale.md), [title](../validators/title.md).

## Options

Options are used with validators and filters: [begins_with](../options/begins-with.md), [boolean](../options/boolean.md),
[ends_with](../options/ends-with.md), [jira](../options/jira.md), [max](../options/max.md), [min](../options/min.md),
[must_exclude](../options/must-exclude.md), [must_include](../options/must-include.md),
[no_empty](../options/no-empty.md), [one_of and none_of](../options/one-of-none-of.md), [required](../options/required.md).

## Operators

Operators help create more complex logic of validations and filters: [and](../operators/and.md),
[or](../operators/or.md), [not](../operators/not.md).

## Actions

Actions that mergeable is able to perform: [assign](../actions/assign.md), [checks](../actions/checks.md),
[close](../actions/close.md), [comment](../actions/comment.md), [labels](../actions/labels.md),
[merge](../actions/merge.md), [request_review](../actions/request-review.md).

## Reusable configuration

YAML has a feature called [anchors](https://yaml.org/spec/1.2.2/#anchors-and-aliases) that allows you to create reusable
parts in the config:

```yaml
on_fail_comment: &default_fail_comment
  - do: comment
    payload:
      body: >
        This issue fails to meet the guidelines, please check the contribution guideline and make sure all the necessary items are in place.

version: 2
mergeable:
  - when: pull_request.*
    validate:
      - do: approvals
        min:
          count: 1
      - do: label
    fail:
      - <<: *default_fail_comment
      - do: assign
        assignees: ['@author']
  - when: issues.*
    validate:
      - do: description
        no_empty:
          enabled: true
    fail: *default_fail_comment
```

A configuration can also extend a configuration in another repository with `_extends`. The value is `repo`,
`owner/repo` or `owner/repo:path/to/file.yml` (the owner defaults to the repository's owner and the path to the same
configuration file). The files are merged: mappings are merged key by key and lists (such as the `mergeable` recipes)
are concatenated, so the recipes of the extended file come first.

```yaml
_extends: my-org/shared-config
version: 2
mergeable:
  - when: pull_request.*
    validate:
      - do: title
        must_exclude:
          regex: 'WIP'
```

## Organisation-wide defaults

You can specify a default configuration to be applied across your GitHub organisation. This can help reduce how many
configuration files you need to maintain and make it easier to get started with Mergeable.

To add a default configuration:

1. Create a repository called `.github` in your organisation.
2. Create a file with the path `.github/mergeable.yml` in this repository.

The final path of the file (including the repository name) is `<YOUR_ORG>/.github/.github/mergeable.yml`.

Mergeable uses this file as the default when it cannot find one in a given repository or pull request. It determines the
file to use in the following order:

1. A `.github/mergeable.yml` changed inside the pull request, if the pull request originates from the same repository.
   If it originates from a fork it is not loaded for security reasons, see
   [mergeability/mergeable#406](https://github.com/mergeability/mergeable/issues/406). This applies to
   `pull_request` and `pull_request_review` events and can be turned off with `use_config_from_pull_request: false`
   in the [settings file](./settings-file.md).
2. A `.github/mergeable.yml` inside the repository the pull request is for.
3. A `.github/mergeable.yml` at `<YOUR_ORG>/.github/.github/mergeable.yml`.

With `use_org_as_default_config` (environment variable `USE_ORG_AS_DEFAULT_CONFIG=true`) the organisation file is
not only a fallback: it is merged with the repository configuration, the organisation recipes first.

:::note

If the configuration file is changed on a base branch by a push, the open pull requests against that branch are
re-run with the changed configuration (event `pull_request.push_synchronize`). This requires the app to have read access to contents and to be
subscribed to the `push` event.

:::

The configuration file name `mergeable.yml` can be changed with the `CONFIG_PATH` environment variable or the
`config_path` setting; the base path is always `.github/`.

### Why the weird default file path?

The default configuration is looked up in a repository named `.github` within the organisation, the same convention
GitHub uses for community health files. The double nesting of `<YOUR_ORG>/.github/.github/mergeable.yml` is necessary
because the GitHub App permissions model lets you specify a single path for the app to access, so it must be the same as
in regular repositories.
