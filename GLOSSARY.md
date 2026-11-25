# Glossary

| Term | Meaning | Rust name |
|---|---|---|
| context | one webhook event: name, payload, installation, GitHub client | `context::Context` |
| flex | the version-2 rule engine | `flex` |
| rule / recipe | one entry of the `mergeable:` list (when, filter, validate, pass, fail, error) | `Value` |
| filter | decides whether a rule applies | `filters::Filter` |
| validator | checks a PR/issue property | `validators::Validator` |
| action | effect after validation | `actions::Action` |
| option (processor) | check inside a setting (must_include, begins_with, ...) | `validators::options`, `filters::options` |
| registry | name -> instance maps, per event | `registry::Registry` |
| interceptor | rewrites the context before the engine (check re-run, milestoned, push) | `interceptors` |
| global settings | .github/mergeable.settings.yml | `settings` |
| configuration | .github/mergeable.yml | `configuration` |
| validation result | {status, name, validations:[...]} | `Value` |
| special annotation | @author, @bot, @sender, @action, @repository | `annotations` |
| case | one recorded webhook scenario with its expected GitHub calls | tests/cases, `contract::case::Case` (app/tests) |
