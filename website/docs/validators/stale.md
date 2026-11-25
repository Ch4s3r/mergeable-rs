---
sidebar_label: stale
---

# Stale

Searches for issues and pull requests that have not been updated for a number of days. The results are passed to the actions (for example a [comment](../actions/comment.md) on every stale item).

```yaml
- do: stale
  days: 20 # number of days ago.
  type: [pull_request, issues] # what items to search for, a list.
  ignore_drafts: true # if set to true, the stale check will ignore draft items
  ignore_milestones: true # if set to true, the stale check will ignore items that have an associated milestone
  ignore_projects: true # if set to true, the stale check will ignore items that have an associated project
  label: # optional property to filter the items that are actioned upon
    match: ['label1_to_match', 'label2_to_match'] # only items with matching labels will be actioned upon and marked as stale
    ignore: ['label1_to_ignore', 'label2_to_ignore'] # items with these labels will be ignored and not marked as stale
  time_constraint: # Optional, run the validator only if it is within the time constraint
    time_zone: 'America/Los_Angeles' # Optional, UTC by default, an IANA time zone name
    hours_between: ['9', '17'] # Optional, 24 hours by default, run only if [0] >= Hour Now <= [1]
    days_of_week: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'] # Optional, 7 days a week by default, specify the days of the week in which to run the validator
```

:::note

This is a special use case. The `schedule.repository` event runs on an interval (enable it with `MERGEABLE_SCHEDULER=true`, see [Scheduler](../operations/scheduler.md)). When used with stale, it searches for issues and/or pull requests that are n days old. See a [full example](../recipes.md#check-stale-pull-requests-and-issues).

:::

:::warning

`type` must be a list. A plain string such as `type: pull_request, issues` is rejected as an unsupported setting (it is expected to be of type array).

:::

## Supported events

```yaml
'schedule.repository'
```
