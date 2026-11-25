---
title: Recipes
slug: /recipes
---

# Recipes

:::tip

Check out the [annotations](./annotations/annotations.md) and [templates](./annotations/templates.md) pages for possible
placeholders in values.

:::

## Work in progress

Prevent accidental merging of pull requests that are work in progress by labeling them `WIP` or prefixing the title with
the abbreviation.

```yaml
version: 2
mergeable:
  - when: pull_request.*
    validate:
      - do: title
        must_exclude:
          regex: ^\[WIP\]
      - do: label
        must_exclude:
          regex: 'wip|work in progress'
```

## No empty description

Ensure all pull requests have a description so that reviewers have context.

```yaml
version: 2
mergeable:
  - when: pull_request.*
    validate:
      - do: description
        no_empty:
          enabled: true
          message: Description matter and should not be empty. Provide detail with **what** was changed, **why** it was changed, and **how** it was changed.
```

## Dependent files

Certain files are related and you want to ensure that they are updated as part of the pull request (i.e. if
`package.json` is updated, so should `yarn.lock` and `package-lock.json`).

```yaml
version: 2
mergeable:
  - when: pull_request.*
    validate:
      - do: dependent
        changed:
          file: 'package.json'  # also supports globs expressions
          required: ['package-lock.json', 'yarn.lock'] # alias: `files` for backward compatibility
```

## Must include milestone

Ensure that all pull requests have a milestone associated. Mergeable also detects when you are
[closing an issue](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/linking-a-pull-request-to-an-issue)
that is associated with the specified milestone.

```yaml
version: 2
mergeable:
  - when: pull_request.*
    validate:
      - do: milestone
        must_include:
          regex: Release 1
```

## Small pull request size

Ensure that pull requests don't exceed a certain size in terms of lines changed (excluding file patterns specified
with `ignore`).

```yaml
version: 2
mergeable:
  - when: pull_request.*
    validate:
      - do: size
        ignore: ['ignore_me.js', 'ignore_this_directory/*', '**/ignore_this_prefix*.js']
        lines:
          max:
            count: 500
            message: Change is very large. Should be under 500 lines of additions and deletions.
```

## Comment if guidelines are not met

Automatically create a comment when a new issue is opened to remind the author when the title does not follow
conventions or is missing a label.

```yaml
version: 2
mergeable:
  - when: issues.opened
    validate:
      - do: title
        begins_with:
          match: ['AUTH', 'SOCIAL', 'CORE']
      - do: label
        must_include:
          regex: bug|enhancement
    fail:
      - do: comment
        payload:
          body: >
            The following problems were found with this issue:
              - Title must begin with `AUTH`, `SOCIAL` or `CORE`
              - The issue should either be labeled `bug` or `enhancement`
```

## Check stale pull requests and issues

Detect issues and pull requests that are n days old (stale) and notify authors and collaborators by creating a comment.
This needs the scheduler (`MERGEABLE_SCHEDULER=true`, see [Scheduler](./operations/scheduler.md)).

```yaml
version: 2
mergeable:
  - when: schedule.repository
    validate:
      - do: stale
        days: 20
        type: [pull_request, issues]
    pass:
      - do: comment
        payload:
          body: This is old. Is it still relevant?
```

## Greet a new contributor

Add a comment on a pull request when it is created.

```yaml
version: 2
mergeable:
  - when: pull_request.opened
    name: "Greet a contributor"
    validate: []
    pass:
      - do: comment
        payload:
          body: >
            Thanks for creating a pull request! A maintainer will review your changes shortly. Please don't be discouraged if it takes a while.
```

## React on a comment to merge pull requests

When a human creates a new comment or edits an existing comment, mergeable looks for a special command in it to then
execute a merge. The comment writer must be different from the pull request author and be a member of a specified team.

```yaml
version: 2
mergeable:
  - when: issue_comment.*
    name: "Merge pull requests when requested via comment"
    filter:
      - do: payload
        sender:
          login:
            must_exclude:
              regex: '\[bot\]$'
    validate:
      - do: lastComment
        must_include:
          regex: 'merge$'
          message: 'Comment "merge" detected, checking eligibility to merge the PR.'
        must_exclude:
          regex: '^\[ \]'
          message: 'Comment contains unchecked items, can''t merge yet.'
        comment_author:
          one_of: ['@org/product-owners-team']
          none_of: ['@author']
    pass:
      - do: merge
        merge_method: "squash"
```

## Auto-merge pull requests once all checks pass

This recipe relies on the main branch being protected so that it only allows merges when the required checks have passed
or the required number of reviews and other conditions are met. Mergeable then merges the pull request as soon as GitHub
shows a green merge button.

Notice the blank validator, which ensures that the merge happens as soon as GitHub allows mergeable to merge the pull
request.

```yaml
version: 2
mergeable:
  - when: pull_request.*, pull_request_review.*, status.*, check_suite.*
    name: "Automatically merge pull requests once it passes all checks"
    validate: []
    pass:
      - do: merge
        merge_method: "squash"
```

## Approval check and title check if certain files are changed

Add two checks to the pull request:

1. Approval check: checks whether the pull request has been approved by certain people.
2. The title should match a regex if certain files are changed. If no changes are made in those files, the check passes.

```yaml
version: 2
mergeable:
  - when: pull_request.*, pull_request_review.*
    name: 'Approval check'
    validate:
      - do: approvals
        min:
          count: 1
        limit:
          users: [ 'approverA', 'approverB' ]

  - when: pull_request.*, pull_request_review.*
    name: 'PR title check'
    validate:
      - do: or
        validate:
          - do: changeset
            must_exclude:
              regex: 'some/regex/for/those/certain/files/*'
          - do: and
            validate:
              - do: changeset
                must_include:
                  regex: 'some/regex/for/those/certain/files/*'
              - do: title
                begins_with:
                  match: [ 'some prefix' ]
```

## Only run rules if the pull request is not a draft

Checks that the pull request's draft state is false before running the validators.

```yaml
version: 2
mergeable:
  - when: pull_request.*, pull_request_review.*
    name: 'Draft check'
    filter:
      - do: payload
        pull_request:
          draft:
            boolean:
              match: false
    validate:
      - do: description
        no_empty:
          enabled: true
          message: Description must be present when PR is not a draft
```

## Allow commits only if they contain an issue ID (like an Azure DevOps work item)

Checks that every commit message begins with an Azure Boards work item ID; otherwise the pull request is commented,
labeled and closed.

```yaml
version: 2
mergeable:
  - when: pull_request.*
    validate:
      - do: commit
        message:
          regex: '^(AB#[0-9]{1,})' # check if all commit messages begin with an AzDO Work Item
    pass:
      - do: comment
        payload:
          body: >
            <h2>Successfully checked for Azure Work Item IDs in commits</h2>
            <h3>All commits in your PR have Azure Board Work Item IDs. Ready for Review!</h3>
            :+1:
      - do: labels
        add: 'Ready for Review'
    fail:
      - do: comment
        payload:
          body: >
            :warning:
            <h2>Azure Boards Work Item IDs missing in commits</h2>
            <h3>Some commits messages were found not having the Azure Boards Work Item ID (AB#1234).</h3>
            <h3>We will close this PR for now.</h3>
            <h3>To resolve, please do one of the following</h3>
            <ul>
              <li>Identify your Azure Boards Work Item ID and <a href="https://gist.github.com/nepsilon/156387acf9e1e72d48fa35c4fabef0b4">amend your commits</a>. Then re-open the PR</li>
              <li>In case you do not have a Work Item ID to reference, please discuss with your reviewer(s) for alternate options</li>
            </ul>
      - do: labels
        add: 'Non-Compliant'
      - do: close
```
