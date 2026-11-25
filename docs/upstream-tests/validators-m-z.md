# Upstream validator tests (m-z) ported to Rust

Source: `__tests__/unit/validators/{milestone,not,or,project,size,stale,title,validator}.test.js` of
mergeability/mergeable (`options_processor/` excluded).

- Upstream tests in area: 76 (the 8 rows of the `stale.test.js` `test.each` table counted one by one)
- Ported: 76 (36 unit tests in `src/validators`, 40 integration tests in `tests/upstream_validators_m_z`)
- Not applicable: 0
- Production fixes: none (every ported test passed against the existing implementation)
- Conflicts with recorded contract cases: none

Notes:

- Unit tests build their context with `crate::filters::test_context`; integration tests call the validator
  directly with a context whose GitHub client points at the contract mock (`tests/contract/mock.rs`), plus a
  local HTTP server for the pull request diff the size validator downloads (`ignore_comments`).
- Stale queries: the mock normalises ISO timestamps in recorded queries, so the search query is compared with
  `updated:<<timestamp>` instead of the exact `now - days` timestamp (that formatting is covered by
  `validators::stale::tests::search_timestamp_has_no_milliseconds_or_zone`).
- Stale `limit option` tests: upstream mocks moment-timezone to a fixed Monday 08:00; the Rust validator reads
  the real clock, so the ported tests derive the configured days, hours and time zones from the current time
  (for example `hours_between` around the current UTC hour, and two time zones 25 hours apart for `time_zone`).

| Upstream test (file > describe > title) | Rust test (path::name) or 'not applicable: reason' |
|---|---|
| milestone.test.js > - > should be false when a different milestone is specified | `mergeable::validators::milestone::tests::fails_when_a_different_milestone_is_required` |
| milestone.test.js > - > shoud be false when milestone is set in settings but null in PR | `mergeable::validators::milestone::tests::fails_when_milestone_is_required_but_pull_request_has_none` |
| milestone.test.js > - > description should be correct | `mergeable::validators::milestone::tests::description_names_the_missing_milestone` |
| milestone.test.js > - > checks that deep validation works if it closes an issue with milestone requirement | `tests/contract_cases.rs > upstream_validators_m_z::milestone::deep_validation_passes_when_the_pull_request_closes_an_issue_with_the_required_milestone` |
| milestone.test.js > - > checks that deep validation return false if it does not closes an issue with milestone requirement | `tests/contract_cases.rs > upstream_validators_m_z::milestone::deep_validation_fails_when_the_closed_issue_has_another_milestone` |
| not.test.js > Not Validator Unit Test > Should pass if subtask fails | `mergeable::validators::not_operator::tests::passes_if_subtask_fails` |
| not.test.js > Not Validator Unit Test > Should fail if subtask passes | `mergeable::validators::not_operator::tests::fails_if_subtask_passes` |
| not.test.js > Not Validator Unit Test > Error is returned when validate has more than one item | `mergeable::validators::not_operator::tests::error_when_validate_has_more_than_one_item` |
| not.test.js > Not Validator Unit Test > Error is returned when validate is missing | `mergeable::validators::not_operator::tests::error_when_validate_is_missing` |
| not.test.js > Not Validator Unit Test > Error is returned when validate is not an array | `mergeable::validators::not_operator::tests::error_when_validate_is_not_an_array` |
| not.test.js > Not Validator Unit Test > Error is returned when validate is empty | `mergeable::validators::not_operator::tests::error_when_validate_is_empty` |
| not.test.js > Not Validator Unit Test > Error is returned when validate uses unsupported classes | `mergeable::validators::not_operator::tests::error_when_validate_uses_unsupported_validators` |
| not.test.js > Not Validator Unit Test > Error if the sub validator errored | `mergeable::validators::not_operator::tests::error_if_a_sub_validator_errored` |
| or.test.js > Or Validator Unit Test > should run subtasks | `mergeable::validators::or_operator::tests::runs_subtasks` |
| or.test.js > Or Validator Unit Test > should return output of second task if first fails | `mergeable::validators::or_operator::tests::passes_with_second_task_when_first_fails` |
| or.test.js > Or Validator Unit Test > should return output of first task to pass when multiple are given | `mergeable::validators::or_operator::tests::passes_with_first_task_when_several_are_given` |
| or.test.js > Or Validator Unit Test > Error is returned when validate is missing | `mergeable::validators::or_operator::tests::error_when_validate_is_missing` |
| or.test.js > Or Validator Unit Test > Error is returned when validate is not an array | `mergeable::validators::or_operator::tests::error_when_validate_is_not_an_array` |
| or.test.js > Or Validator Unit Test > Error is returned when validate is empty | `mergeable::validators::or_operator::tests::error_when_validate_is_empty` |
| or.test.js > Or Validator Unit Test > Error is returned when validate uses unsupported classes | `mergeable::validators::or_operator::tests::error_when_validate_uses_unsupported_validators` |
| or.test.js > Or Validator Unit Test > Supports nested or validator | `mergeable::validators::or_operator::tests::supports_nested_or_validator` |
| or.test.js > Or Validator Unit Test > error status if one of the sub validator errored | `mergeable::validators::or_operator::tests::error_if_one_of_the_sub_validators_errored` |
| project.test.js > - > that mergeable is true when PR number is in Project | `tests/contract_cases.rs > upstream_validators_m_z::project::pull_request_on_the_project_board_is_mergeable` |
| project.test.js > - > that mergeable is false when PR number is not in Project | `tests/contract_cases.rs > upstream_validators_m_z::project::pull_request_missing_from_the_project_board_is_not_mergeable` |
| project.test.js > - > test description is correct | `tests/contract_cases.rs > upstream_validators_m_z::project::missing_project_description_names_the_required_project` |
| project.test.js > - > test deep validation works | `tests/contract_cases.rs > upstream_validators_m_z::project::deep_validation_passes_when_the_pull_request_closes_an_issue_on_the_board` |
| size.test.js > PR size validator > errors if both max and total are passed | `tests/contract_cases.rs > upstream_validators_m_z::size::max_and_total_together_are_an_error` |
| size.test.js > PR size validator > backwards compatibility test for max and total | `tests/contract_cases.rs > upstream_validators_m_z::size::max_stays_backwards_compatible_as_total` |
| size.test.js > PR size validator > errors if invalid configurations is passed | `tests/contract_cases.rs > upstream_validators_m_z::size::missing_lines_configuration_is_an_error` |
| size.test.js > PR size validator > fails when size of changes is above count | `tests/contract_cases.rs > upstream_validators_m_z::size::fails_when_size_of_changes_is_above_count` |
| size.test.js > PR size validator > passes when changes equal to count | `tests/contract_cases.rs > upstream_validators_m_z::size::passes_when_changes_equal_the_count` |
| size.test.js > PR size validator > partial fails and passing when part of PR is within count | `tests/contract_cases.rs > upstream_validators_m_z::size::fails_partially_when_only_part_of_the_pull_request_is_within_count` |
| size.test.js > PR size validator > ignores specified files | `tests/contract_cases.rs > upstream_validators_m_z::size::ignores_specified_files` |
| size.test.js > PR size validator > match hidden files | `tests/contract_cases.rs > upstream_validators_m_z::size::matches_hidden_files` |
| size.test.js > PR size validator > ignores glob patterns | `tests/contract_cases.rs > upstream_validators_m_z::size::ignores_and_matches_glob_patterns` |
| size.test.js > PR size validator > handles empty ignore args | `tests/contract_cases.rs > upstream_validators_m_z::size::handles_empty_ignore_list` |
| size.test.js > Size Ignore comment functionality > both single line comments and block comments are ignored | `tests/contract_cases.rs > upstream_validators_m_z::size::both_single_line_comments_and_block_comments_are_ignored` |
| size.test.js > Size Ignore comment functionality > non supported file extension are ignored | `tests/contract_cases.rs > upstream_validators_m_z::size::comments_in_unsupported_file_extensions_are_counted` |
| size.test.js > Size Ignore comment functionality > .js file extension works properly | `tests/contract_cases.rs > upstream_validators_m_z::size::js_file_extension_ignores_line_and_block_comments` |
| size.test.js > Size Ignore comment functionality > .py file extension works properly | `tests/contract_cases.rs > upstream_validators_m_z::size::py_file_extension_ignores_hash_comments` |
| stale.test.js > - > will set the issues and pulls appropriately when both types are specified | `tests/contract_cases.rs > upstream_validators_m_z::stale::both_types_search_issues_and_pull_requests_together` |
| stale.test.js > - > will set the issues and pulls appropriately when no type is set | `tests/contract_cases.rs > upstream_validators_m_z::stale::no_type_searches_issues_and_pull_requests_together` |
| stale.test.js > metadata queries > will set %s correctly (test.each row { draft: true, milestones: true, projects: true }) | `tests/contract_cases.rs > upstream_validators_m_z::stale::metadata_query_ignores_drafts_milestones_and_projects` |
| stale.test.js > metadata queries > will set %s correctly (test.each row { draft: false, milestones: true, projects: true }) | `tests/contract_cases.rs > upstream_validators_m_z::stale::metadata_query_ignores_milestones_and_projects` |
| stale.test.js > metadata queries > will set %s correctly (test.each row { draft: true, milestones: false, projects: true }) | `tests/contract_cases.rs > upstream_validators_m_z::stale::metadata_query_ignores_drafts_and_projects` |
| stale.test.js > metadata queries > will set %s correctly (test.each row { draft: true, milestones: true, projects: false }) | `tests/contract_cases.rs > upstream_validators_m_z::stale::metadata_query_ignores_drafts_and_milestones` |
| stale.test.js > metadata queries > will set %s correctly (test.each row { draft: false, milestones: false, projects: true }) | `tests/contract_cases.rs > upstream_validators_m_z::stale::metadata_query_ignores_projects_only` |
| stale.test.js > metadata queries > will set %s correctly (test.each row { draft: true, milestones: false, projects: false }) | `tests/contract_cases.rs > upstream_validators_m_z::stale::metadata_query_ignores_drafts_only` |
| stale.test.js > metadata queries > will set %s correctly (test.each row { draft: false, milestones: true, projects: false }) | `tests/contract_cases.rs > upstream_validators_m_z::stale::metadata_query_ignores_milestones_only` |
| stale.test.js > metadata queries > will set %s correctly (test.each row { draft: false, milestones: false, projects: false }) | `tests/contract_cases.rs > upstream_validators_m_z::stale::metadata_query_ignores_nothing` |
| stale.test.js > label queries > will set label matches and ignores correctly | `tests/contract_cases.rs > upstream_validators_m_z::stale::label_matches_and_ignores_become_label_qualifiers` |
| stale.test.js > label queries > will set label matches correctly | `tests/contract_cases.rs > upstream_validators_m_z::stale::label_matches_become_label_qualifiers` |
| stale.test.js > label queries > will set label ignores correctly | `tests/contract_cases.rs > upstream_validators_m_z::stale::label_ignores_become_negated_label_qualifiers` |
| stale.test.js > label queries > will not add any label queries when no label is specified in match settings | `tests/contract_cases.rs > upstream_validators_m_z::stale::no_label_settings_add_no_label_qualifiers` |
| stale.test.js > - > will set the issues and pulls even when unsupported type is set | `tests/contract_cases.rs > upstream_validators_m_z::stale::unsupported_types_still_schedule_issues_and_pulls` |
| stale.test.js > - > will set the issues and pulls correctly when type is issue only | `tests/contract_cases.rs > upstream_validators_m_z::stale::issues_type_only_searches_issues` |
| stale.test.js > - > will set the issues and pulls correctly when type is pull_request only | `tests/contract_cases.rs > upstream_validators_m_z::stale::pull_request_type_only_searches_pull_requests` |
| stale.test.js > limit option > time_zone option works correctly | `tests/contract_cases.rs > upstream_validators_m_z::stale::time_zone_option_decides_the_local_day` |
| stale.test.js > limit option > days_of_week option works correctly | `tests/contract_cases.rs > upstream_validators_m_z::stale::days_of_week_option_only_runs_on_listed_days` |
| stale.test.js > limit option > hours_between option works correctly | `tests/contract_cases.rs > upstream_validators_m_z::stale::hours_between_option_only_runs_within_the_hours` |
| title.test.js > - > validate returns false | `mergeable::validators::title::tests::validate_returns_fail_for_wip_and_pass_for_conventional_prefix` |
| title.test.js > - > fail gracefully if invalid regex | `mergeable::validators::title::tests::odd_exclude_regex_fails_gracefully_and_passes` |
| title.test.js > - > checks that it fail when exclude regex is in title | `mergeable::validators::title::tests::fails_when_exclude_regex_is_in_title` |
| title.test.js > - > checks that advance setting of must_include works | `mergeable::validators::title::tests::must_include_message_replaces_the_default_description` |
| title.test.js > begins_with > checks that it fail when begins_with is not in title | `mergeable::validators::title::tests::begins_with_fails_when_title_does_not_begin_with_match` |
| title.test.js > begins_with > with match as arrays | `mergeable::validators::title::tests::begins_with_accepts_any_match_of_an_array` |
| title.test.js > ends_with > checks that it fail when ends_with is not in title | `mergeable::validators::title::tests::ends_with_fails_when_title_does_not_end_with_match` |
| title.test.js > ends_with > with array | `mergeable::validators::title::tests::ends_with_accepts_any_match_of_an_array` |
| title.test.js > - > checks that it fail when include regex is in title | `mergeable::validators::title::tests::fails_when_include_regex_is_missing_from_title` |
| validator.test.js > Validator#isEventSupported > Returns correctly with one supported event | `mergeable::validators::tests::event_support_with_one_supported_event` |
| validator.test.js > Validator#isEventSupported > Returns correctly with more than one supported event | `mergeable::validators::tests::event_support_with_several_supported_events` |
| validator.test.js > Validator#isEventSupported > Returns correctly with a wildcard in the event name | `mergeable::validators::tests::event_support_with_a_wildcard_in_the_event_name` |
| validator.test.js > Validator#validateSetting > throw error if the type mismatch | `mergeable::validators::tests::setting_of_the_wrong_type_is_rejected` |
| validator.test.js > Validator#validateSetting > throw error if unknown type | `mergeable::validators::tests::unknown_setting_is_rejected` |
| validator.test.js > Validator#validateSetting > nested settings work | `mergeable::validators::tests::nested_settings_are_validated` |
| validator.test.js > Validator#validateSetting > multiple settings type work | `mergeable::validators::tests::setting_may_allow_several_types` |
