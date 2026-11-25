---
title: Supported events
---

# Supported events

The `when` of a recipe names one or more webhook events as `<event>.<action>`, for example `pull_request.opened`, or
`<event>.*` for every action of an event. Several events are separated by commas:
`when: pull_request.*, pull_request_review.*`. The scheduler adds the event `schedule.repository` (see
[Scheduler](../operations/scheduler.md)).

A validator, filter or action only runs for the events it supports. The operators `and`, `or` and `not` support every
event.

## Validators

| Validator | Events |
|---|---|
| [age](../validators/age.md) | `pull_request.*`, `pull_request_review.*`, `issue_comment.*` |
| [approvals](../validators/approvals.md) | `pull_request.*`, `pull_request_review.*` |
| [assignee](../validators/assignee.md) | `pull_request.*`, `pull_request_review.*`, `issues.*`, `issue_comment.*` |
| [author](../validators/author.md) | `pull_request.*`, `pull_request_review.*`, `issues.*`, `issue_comment.*` |
| [baseRef](../validators/base-ref.md) | `pull_request.*`, `pull_request_review.*`, `check_suite.*`, `status.*` |
| [changeset](../validators/changeset.md) | `pull_request.*`, `pull_request_review.*` |
| [commit](../validators/commit.md) | `pull_request.*`, `pull_request_review.*` |
| [contents](../validators/contents.md) | `pull_request.*`, `pull_request_review.*` |
| [dependent](../validators/dependent.md) | `pull_request.*`, `pull_request_review.*` |
| [description](../validators/description.md) | `pull_request.*`, `pull_request_review.*`, `issues.*`, `issue_comment.*` |
| [headRef](../validators/head-ref.md) | `pull_request.*`, `pull_request_review.*` |
| [label](../validators/label.md) | `pull_request.*`, `pull_request_review.*`, `issues.*`, `issue_comment.*` |
| [lastComment](../validators/last-comment.md) | `pull_request.*`, `pull_request_review.*`, `issues.*`, `issue_comment.created`, `issue_comment.edited` |
| [milestone](../validators/milestone.md) | `pull_request.*`, `pull_request_review.*`, `issues.*` |
| [project](../validators/project.md) | `pull_request.*`, `pull_request_review.*`, `issues.*` |
| [size](../validators/size.md) | `pull_request.*`, `pull_request_review.*` |
| [stale](../validators/stale.md) | `schedule.repository` |
| [title](../validators/title.md) | `pull_request.*`, `pull_request_review.*`, `issues.*`, `issue_comment.*` |

## Filters

| Filter | Events |
|---|---|
| [author](../filters/author.md) | `pull_request.*`, `pull_request_review.*` |
| [baseRef](../filters/base-ref.md) | `pull_request.*`, `pull_request_review.*` |
| [payload](../filters/payload.md) | `pull_request.*`, `pull_request_review.*`, `issues.*`, `issue_comment.*` |
| [repository](../filters/repository.md) | `pull_request.*`, `pull_request_review.*` |

## Actions

| Action | Events |
|---|---|
| [assign](../actions/assign.md) | `pull_request.*`, `issues.*`, `issue_comment.*` |
| [checks](../actions/checks.md) | the `pull_request` actions listed on its page (not `pull_request.closed`), `pull_request_review.dismissed`, `pull_request_review.edited`, `pull_request_review.submitted`, `issue_comment.*` |
| [close](../actions/close.md) | `pull_request.*`, `issues.*`, `issue_comment.*`, `schedule.repository` |
| [comment](../actions/comment.md) | `pull_request.*`, `issues.*`, `issue_comment.*`, `schedule.repository` |
| [labels](../actions/labels.md) | `pull_request.*`, `issues.*`, `issue_comment.*`, `schedule.repository` |
| [merge](../actions/merge.md) | `pull_request.*`, `pull_request_review.*`, `status.*`, `check_suite.*`, `issue_comment.*` |
| [request_review](../actions/request-review.md) | `pull_request.*` |
