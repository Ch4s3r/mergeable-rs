---
title: Templates
---

# Templates

The texts of the [comment](../actions/comment.md) and [checks](../actions/checks.md) actions and the `commit_title` and
`commit_message` of the [merge](../actions/merge.md) action are [Handlebars](https://handlebarsjs.com) templates.
mergeable-rs renders them with its own template engine, which reproduces the output of the original implementation
(including Handlebars' HTML escaping: use triple braces `{{{ value }}}` to insert a value unescaped).

## Data

In the comment and checks actions a template can use:

- the fields of the pull request or issue of the event, for example `{{title}}`, `{{body}}`, `{{number}}`,
  `{{user.login}}`, `{{created_at}}`,
- the validation result:

| Variable | Meaning |
|---|---|
| `validationStatus` | the overall status: `pass`, `fail` or `error` |
| `validationCount` | the number of validators that ran |
| `passCount`, `failCount`, `errorCount` | the number of validators per status |
| `validationSuites` | the result of every validator: `name`, `status` and `validations` (each with `status`, `description` and `details` such as `input`, `settings` and `error`) |
| `failures` | the validation suites that failed |

In the merge action, `commit_title` and `commit_message` are rendered with the pull request returned by
[`GET /repos/{owner}/{repo}/pulls/{pull_number}`](https://docs.github.com/en/rest/pulls/pulls#get-a-pull-request).

## Helpers

Besides the Handlebars built-ins (`if`, `unless`, `each`, `with`, `lookup`) these helpers are available:

| Helper | Output |
|---|---|
| `{{toUpperCase value}}` | the text in upper case |
| `{{formatDate}}` | the current date and time, e.g. `Jun 20, 2024, 2:05 PM` (UTC) |
| `{{formatDate created_at}}` | the given date (ISO 8601 or RFC 2822) in the same format, or `Invalid Date` |
| `{{statusIcon status}}` | an emoji for a status: `:heavy_check_mark:` (pass), `:x:` (fail), `:heavy_exclamation_mark:` (error), `:information_source:` (info) |
| `{{displaySettings settings}}` | the settings as JSON |
| `{{breaklines text}}` | the text with line breaks turned into `<br>` |
| `{{#ifEquals a b}}...{{/ifEquals}}` | the block if both values are equal |

## Example

```yaml
- do: comment
  payload:
    body: >
      Hi @author, {{failCount}} of {{validationCount}} checks failed on "{{title}}" ({{formatDate}}):
      {{#each failures}} {{toUpperCase name}}{{/each}}
```
