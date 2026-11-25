---
sidebar_label: jira
---

# Jira

`jira` extracts a ticket key from the input with a regex and checks through the Jira REST API that the ticket exists.

:::note

The option needs the `JIRA_*` environment variables of your instance (below). The Jira request uses basic authentication and always verifies TLS certificates.

:::

| Env             | Description                                | Required | Default |
|-----------------|--------------------------------------------|----------|---------|
| JIRA_PASSWORD   | Password to authenticate with JIRA         | Yes      |         |
| JIRA_USERNAME   | Username to authenticate with JIRA         | Yes      |         |
| JIRA_HOST       | Host to authenticate with JIRA             | yes      |         |
| JIRA_PROTOCOL   | Protocol to establish connection with JIRA | no       | https   |
| JIRA_VERSION    | JIRA API Version to use                    | no       | 2       |
| JIRA_STRICT_SSL | Accepted for compatibility, has no effect  | no       |         |


```yaml
- do: headRef
  jira:
    regex: '[A-Z][A-Z0-9]+-\d+'
    regex_flag: none
    message: 'The Jira ticket is not valid'
```

| Param | Description | Required | Default Message |
|----|----|----|----|
| regex | Regex enabled message to validate input with | Yes |  |
| message | Message to show if the validation fails | No | \[INPUT NAME\] does not include \[REGEX\] |
| regex_flag | Regex flag to be used with regex param to validate inputs | No | i |

## Supported validators

```yaml
'commit', 'description', 'headRef', 'label', 'milestone', 'title'
```
