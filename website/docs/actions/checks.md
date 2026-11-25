---
sidebar_label: checks
---

# Checks

You can add check runs to a pull request to enforce that it can only be merged once all checks returned with success.

:::note

Whether a `checks` action is added by default:

1. If no action is provided in either `pass`, `fail` or `error`, `checks` is added as default (to be backward
   compatible).
2. If only actions other than `checks` are provided, no check is added (to support cases where checks are not wanted).
3. If `checks` is part of the actions provided, all `pass`, `fail` and `error` cases get `checks` (to prevent a check
   that hangs and never finishes processing).

:::

When a recipe starts, the check run is created with the status `in_progress` (title `Mergeable is running.`) and it is
updated with the outcome once the validation is done. The check run is named `Mergeable`, or `Mergeable: <name>` for a
recipe with a `name`.

```yaml
- do: checks
  state: 'completed' # Optional, default is 'completed'. Another state, e.g. 'in_progress', leaves the check pending.
  status: 'success' # The conclusion of a completed check. Can be: success, failure, neutral, cancelled, timed_out, or action_required
  payload:
    title: 'Mergeable Run has been Completed!'
    summary: 'All the validators have returned pass!'
    text: 'Optional details' # Optional
```

The payload supports [templates](../annotations/templates.md) and [annotations](../annotations/annotations.md). These
are the defaults used for missing fields:

```yaml
- do: checks # default pass case
  status: 'success'
  payload:
    title: 'Mergeable Run has been Completed!'
    summary: "All the validators have returned 'pass'! \n Here are some stats of the run: \n {{validationCount}} validations were run"

- do: checks # default fail case
  status: 'failure'
  payload:
    title: "{{failCount}}/{{validationCount}} Fail(s): {{#each failures}} {{toUpperCase name}}{{^@last}}, {{/@last}}{{/each}}"
    summary: |
      ### Status: {{toUpperCase validationStatus}}

      Here are some stats of the run:
      {{validationCount}} validations were run.
      {{passCount}} PASSED
      {{failCount}} FAILED
    text: |
      {{#each validationSuites}}
      #### {{{statusIcon status}}} Validator: {{toUpperCase name}}
      {{#each validations }} * {{{statusIcon status}}} ***{{{ description }}}***
             Input : {{{details.input}}}
             Settings : {{{displaySettings details.settings}}}
             {{/each}}
      {{/each}}

- do: checks # default error case
  status: 'action_required'
  payload:
    title: 'Mergeable found some failed checks!'
    summary: |
      ### Status: {{toUpperCase validationStatus}}
      Some or All of the validators have returned 'error' status, please check below for details

      Here are some stats of the run:
      {{validationCount}} validations were run.
      {{passCount}} ***PASSED***
      {{failCount}} ***FAILED***
      {{errorCount}} ***ERRORED***
    text: |
      {{#each validationSuites}}
      #### {{{statusIcon status}}} Validator: {{toUpperCase name}}
      Status {{toUpperCase status}}
      {{#each validations }} * {{{statusIcon status}}} ***{{{ description }}}***
             Input : {{{details.input}}}
             Settings : {{{displaySettings details.settings}}}
             {{#if details.error}}
             Error : {{{details.error}}}
             {{/if}}
             {{/each}}
      {{/each}}
```

:::note

If any of the fields `title`, `summary` or `status` is missing, the default values are used.

:::

:::note

Checks are re-run automatically if the configuration file is changed on the base branch, see
[organisation-wide defaults](../configuration/configuration.md#organisation-wide-defaults).

:::

## Supported events

The `pull_request.closed` event is not supported since it has no meaningful use in the context of the GitHub checks API.

```yaml
'pull_request.assigned', 'pull_request.auto_merge_disabled', 'pull_request.auto_merge_enabled',
'pull_request.converted_to_draft', 'pull_request.demilestoned', 'pull_request.dequeued', 'pull_request.edited',
'pull_request.enqueued', 'pull_request.labeled', 'pull_request.locked', 'pull_request.milestoned',
'pull_request.opened', 'pull_request.push_synchronize', 'pull_request.ready_for_review', 'pull_request.reopened',
'pull_request.review_request_removed', 'pull_request.review_requested', 'pull_request.synchronize',
'pull_request.unassigned', 'pull_request.unlabeled', 'pull_request.unlocked', 'pull_request_review.dismissed',
'pull_request_review.edited', 'pull_request_review.submitted', 'issue_comment.*'
```
