# Upstream tests: filters, flex, interceptors, settings, metaData, mergeable

Upstream: `mergeability/mergeable` `__tests__/unit/{filters,flex,interceptors,settings}/`, `__tests__/unit/metaData.test.js`,
`__tests__/unit/mergeable.test.js`.

- Upstream tests in this area: 117
- Ported: 117 (57 unit tests in `src/`, 60 integration tests in `tests/upstream_filters_flex/mod.rs`, run by `tests/contract_cases.rs`)
- Not applicable: 0

Rust paths without a prefix are unit tests in the `mergeable` library (`src/`); paths starting with
`upstream_filters_flex::` are integration tests against the mock GitHub API (`tests/contract/mock.rs`).

| Upstream test (file > describe > title) | Rust test |
|---|---|
| filters/filter.test.js > Filter#isEventSupported > Returns correctly with one supported event | `filters::test_context::filter_base_tests::one_supported_event_matches_only_that_event` |
| filters/filter.test.js > Filter#isEventSupported > Returns correctly with more than one supported event | `filters::test_context::filter_base_tests::several_supported_events_match_each_listed_event` |
| filters/filter.test.js > Filter#isEventSupported > Returns correctly with a wildcard in the event name | `filters::test_context::filter_base_tests::wildcard_supported_event_matches_every_action_of_the_event` |
| filters/filter.test.js > Filter#validateSetting > throw error if the type mismatch | `filters::test_context::filter_base_tests::mismatching_setting_type_names_the_expected_type` |
| filters/filter.test.js > Filter#validateSetting > throw error if unknown type | `filters::test_context::filter_base_tests::unknown_setting_is_not_supported` |
| filters/filter.test.js > Filter#validateSetting > nested settings work | `filters::test_context::filter_base_tests::nested_unknown_setting_names_its_path` |
| filters/filter.test.js > Filter#validateSetting > multiple settings type work | `filters::test_context::filter_base_tests::setting_may_allow_several_types` |
| filters/and.test.js > And Filter Unit Test > should run subtasks | `upstream_filters_flex::and_filter::and_runs_its_sub_filters` |
| filters/and.test.js > And Filter Unit Test > should return output of second task if first fails | `upstream_filters_flex::and_filter::and_fails_when_the_first_sub_filter_fails_and_the_second_passes` |
| filters/and.test.js > And Filter Unit Test > should return output of first task to pass when multiple are given | `upstream_filters_flex::and_filter::and_passes_when_every_sub_filter_passes` |
| filters/and.test.js > And Filter Unit Test > Error is returned when filter is missing | `upstream_filters_flex::and_filter::and_without_filter_option_is_an_error` |
| filters/and.test.js > And Filter Unit Test > Error is returned when filter is not an array | `upstream_filters_flex::and_filter::and_with_non_list_filter_option_is_an_error` |
| filters/and.test.js > And Filter Unit Test > Error is returned when filter is empty | `upstream_filters_flex::and_filter::and_with_empty_filter_option_is_an_error` |
| filters/and.test.js > And Filter Unit Test > Error is returned when filter uses unsupported classes | `upstream_filters_flex::and_filter::and_with_unknown_sub_filter_is_an_error` |
| filters/and.test.js > And Filter Unit Test > Supports nested and filter | `upstream_filters_flex::and_filter::and_supports_a_nested_or_filter` |
| filters/and.test.js > And Filter Unit Test > error if one of the sub filter errored | `upstream_filters_flex::and_filter::and_errors_when_a_nested_sub_filter_errors` |
| filters/or.test.js > Or Filter Unit Test > should run subtasks | `upstream_filters_flex::or_filter::or_runs_its_sub_filters` |
| filters/or.test.js > Or Filter Unit Test > should return output of second task if first fails | `upstream_filters_flex::or_filter::or_passes_when_the_first_sub_filter_fails_and_the_second_passes` |
| filters/or.test.js > Or Filter Unit Test > should return output of first task to pass when multiple are given | `upstream_filters_flex::or_filter::or_passes_when_the_first_sub_filter_passes` |
| filters/or.test.js > Or Filter Unit Test > Error is returned when validate is missing | `upstream_filters_flex::or_filter::or_without_filter_option_is_an_error` |
| filters/or.test.js > Or Filter Unit Test > Error is returned when validate is not an array | `upstream_filters_flex::or_filter::or_with_non_list_filter_option_is_an_error` |
| filters/or.test.js > Or Filter Unit Test > Error is returned when validate is empty | `upstream_filters_flex::or_filter::or_with_empty_filter_option_is_an_error` |
| filters/or.test.js > Or Filter Unit Test > Error is returned when validate uses unsupported classes | `upstream_filters_flex::or_filter::or_with_unknown_sub_filter_is_an_error` |
| filters/or.test.js > Or Filter Unit Test > Supports nested or validator | `upstream_filters_flex::or_filter::or_supports_a_nested_or_filter` |
| filters/or.test.js > Or Filter Unit Test > error status if one of the sub validator errored | `upstream_filters_flex::or_filter::or_errors_when_a_nested_sub_filter_errors` |
| filters/not.test.js > Not Filter Unit Test > Should pass if subtasks fails | `upstream_filters_flex::not_filter::not_passes_when_its_sub_filter_fails` |
| filters/not.test.js > Not Filter Unit Test > Should fail if subtasks passes | `upstream_filters_flex::not_filter::not_fails_when_its_sub_filter_passes` |
| filters/not.test.js > Not Filter Unit Test > Error is returned when filter has more than one item | `upstream_filters_flex::not_filter::not_with_more_than_one_sub_filter_is_an_error` |
| filters/not.test.js > Not Filter Unit Test > Error is returned when filter is missing | `upstream_filters_flex::not_filter::not_without_filter_option_is_an_error` |
| filters/not.test.js > Not Filter Unit Test > Error is returned when filter is not an array | `upstream_filters_flex::not_filter::not_with_non_list_filter_option_is_an_error` |
| filters/not.test.js > Not Filter Unit Test > Error is returned when filter is empty | `upstream_filters_flex::not_filter::not_with_empty_filter_option_is_an_error` |
| filters/not.test.js > Not Filter Unit Test > Error is returned when filter uses unsupported classes | `upstream_filters_flex::not_filter::not_with_unknown_sub_filter_is_an_error` |
| filters/not.test.js > Not Filter Unit Test > Error if one of the sub filters errored | `upstream_filters_flex::not_filter::not_errors_when_a_nested_sub_filter_errors` |
| filters/baseRef.test.js > (top level) > should fail with unexpected baseRef | `filters::base_ref::tests::base_ref_not_matching_must_include_fails` |
| filters/baseRef.test.js > (top level) > should pass with expected baseRef | `filters::base_ref::tests::base_ref_matching_must_include_passes` |
| filters/baseRef.test.js > (top level) > should fail with excluded baseRef | `filters::base_ref::tests::base_ref_matching_must_exclude_fails` |
| filters/baseRef.test.js > (top level) > should pass with excluded baseRef | `filters::base_ref::tests::base_ref_not_matching_must_exclude_passes` |
| filters/author.test.js > (top level) > should fail with unexpected author | `filters::author::tests::unexpected_author_fails_must_include` |
| filters/author.test.js > (top level) > should fail with unexpected author one_of author | `filters::author::tests::author_missing_from_one_of_list_fails` |
| filters/author.test.js > (top level) > should pass with expected author | `filters::author::tests::expected_author_passes_must_include` |
| filters/author.test.js > (top level) > should pass with one_of author | `filters::author::tests::author_in_one_of_list_passes` |
| filters/author.test.js > (top level) > should fail with excluded author | `filters::author::tests::excluded_author_fails_must_exclude` |
| filters/author.test.js > (top level) > should fail with none_of author | `filters::author::tests::author_in_none_of_list_fails` |
| filters/author.test.js > (top level) > should pass with none_of author | `filters::author::tests::author_missing_from_none_of_list_passes` |
| filters/author.test.js > (top level) > should pass with expected author from correct team | `upstream_filters_flex::author_filter_teams::expected_author_in_the_team_passes` |
| filters/author.test.js > (top level) > should pass with one_of author from correct team | `upstream_filters_flex::author_filter_teams::expected_author_one_of_the_team_members_passes` |
| filters/author.test.js > (top level) > should fail with expected author from incorrect team | `upstream_filters_flex::author_filter_teams::expected_author_outside_the_team_fails` |
| filters/author.test.js > (top level) > should fail with one_of author from incorrect team | `upstream_filters_flex::author_filter_teams::expected_author_not_one_of_the_empty_team_fails` |
| filters/author.test.js > (top level) > should fail with unexpected author from correct team | `upstream_filters_flex::author_filter_teams::unexpected_author_in_the_team_fails` |
| filters/author.test.js > (top level) > should fail with one_of author from correct team | `upstream_filters_flex::author_filter_teams::unexpected_author_one_of_the_team_members_fails` |
| filters/author.test.js > (top level) > should pass when the author is a member of the team | `upstream_filters_flex::author_filter_teams::author_who_is_a_team_member_passes_the_team_option` |
| filters/author.test.js > (top level) > should pass when the author is one_of the members of the team | `upstream_filters_flex::author_filter_teams::author_one_of_the_team_members_passes` |
| filters/author.test.js > (top level) > should fail when the author is not a member of the team | `upstream_filters_flex::author_filter_teams::author_who_is_not_a_team_member_fails_the_team_option` |
| filters/author.test.js > (top level) > should fail when the author is not one_of the members of the team | `upstream_filters_flex::author_filter_teams::author_not_one_of_the_team_members_fails` |
| filters/author.test.js > (top level) > should pass when the author is not member of the none_of team | `upstream_filters_flex::author_filter_teams::author_outside_the_none_of_team_passes` |
| filters/author.test.js > (top level) > should fail when the author is member of the none_of team | `upstream_filters_flex::author_filter_teams::author_in_the_none_of_team_fails` |
| filters/author.test.js > (top level) > should pass when the author is one_of @author | `filters::author::tests::author_annotation_in_one_of_passes` |
| filters/author.test.js > (top level) > should fail when the author is none_of @author | `filters::author::tests::author_annotation_in_none_of_fails` |
| filters/repository.test.js > (top level) > fail gracefully if invalid regex | `upstream_filters_flex::repository_filter_topics::pattern_that_never_matches_passes_must_exclude_gracefully` |
| filters/repository.test.js > (top level) > public repo with private filter | `filters::repository::tests::public_repository_fails_private_visibility` |
| filters/repository.test.js > (top level) > public repo with public filter | `filters::repository::tests::public_repository_passes_public_visibility` |
| filters/repository.test.js > (top level) > private repo with public filter | `filters::repository::tests::private_repository_fails_public_visibility` |
| filters/repository.test.js > (top level) > private repo with private filter | `filters::repository::tests::private_repository_passes_private_visibility` |
| filters/repository.test.js > (top level) > must include topic | `upstream_filters_flex::repository_filter_topics::topic_matching_must_include_passes` |
| filters/repository.test.js > (top level) > fail to must include topic | `upstream_filters_flex::repository_filter_topics::missing_topic_fails_must_include` |
| filters/repository.test.js > (top level) > must exclude topic | `upstream_filters_flex::repository_filter_topics::missing_topic_passes_must_exclude` |
| filters/repository.test.js > (top level) > fail to must exclude topic | `upstream_filters_flex::repository_filter_topics::topic_matching_must_exclude_fails` |
| filters/repository.test.js > (top level) > fail to must include name | `filters::repository::tests::repository_name_not_matching_must_include_fails` |
| filters/repository.test.js > (top level) > must_include name passes | `filters::repository::tests::repository_name_matching_must_include_passes` |
| filters/repository.test.js > (top level) > fail to must exclude name | `filters::repository::tests::repository_name_matching_must_exclude_fails` |
| filters/repository.test.js > (top level) > must_exclude name passes | `filters::repository::tests::repository_name_not_matching_must_exclude_passes` |
| filters/payload.test.js > (top level) > that checks are done against correct field | `filters::payload::tests::checks_run_against_the_configured_field` |
| filters/payload.test.js > (top level) > that proper errors are returned if the field does not exist | `filters::payload::tests::missing_field_is_reported_as_error` |
| filters/payload.test.js > (top level) > that checks work against objects in an array with must_include | `filters::payload::tests::must_include_checks_objects_in_an_array_by_key` |
| filters/payload.test.js > (top level) > that checks work against objects in an array with must_exclude | `filters::payload::tests::must_exclude_checks_objects_in_an_array_by_key` |
| filters/payload.test.js > (top level) > that proper errors are returned if a key is required but not provided in must_include | `filters::payload::tests::must_include_on_objects_without_key_is_an_error` |
| filters/payload.test.js > (top level) > that proper errors are returned if a key is required but not provided in must_exclude | `filters::payload::tests::must_exclude_on_objects_without_key_is_an_error` |
| filters/payload.test.js > (top level) > that boolean checks succeed | `filters::payload::tests::boolean_check_follows_the_draft_flag` |
| filters/options_processor/options/boolean.test.js > (top level) > return pass if input is a string that meets the criteria | `filters::options::boolean::tests::string_input_matching_the_expected_boolean_passes` |
| filters/options_processor/options/boolean.test.js > (top level) > return pass if input is a boolean that meets the criteria | `filters::options::boolean::tests::boolean_input_matching_the_expected_boolean_passes` |
| filters/options_processor/options/boolean.test.js > (top level) > return fail if input does not meet the criteria | `filters::options::boolean::tests::input_not_matching_the_expected_boolean_fails` |
| filters/options_processor/options/boolean.test.js > (top level) > return error if inputs are not in expected format | `filters::options::boolean::tests::input_that_is_not_a_boolean_is_rejected` |
| flex/flex.test.js > Test processBeforeValidate and processAfterValidate invocations > when event is in configuration | `workflow::flex_tests::event_in_configuration_runs_actions_before_and_after_validation` |
| flex/flex.test.js > Test processBeforeValidate and processAfterValidate invocations > when event is not in configuration | `workflow::flex_tests::event_not_in_configuration_runs_no_actions` |
| flex/flex.test.js > Test processBeforeValidate and processAfterValidate invocations > when event is in configuration with multiple whens | `workflow::flex_tests::event_matching_one_of_multiple_whens_runs_its_actions_once` |
| flex/flex.test.js > Test processBeforeValidate and processAfterValidate invocations > two whens with same events | `workflow::flex_tests::two_whens_with_the_same_event_run_actions_once_per_rule` |
| flex/flex.test.js > Test processBeforeValidate and processAfterValidate invocations > Expect pass action to be triggered on empty validator | `upstream_filters_flex::flex_executor::pass_action_runs_for_a_rule_without_validators` |
| flex/flex.test.js > Test processBeforeValidate and processAfterValidate invocations > Expect fail action to not be triggered on empty validator | `upstream_filters_flex::flex_executor::fail_action_does_not_run_for_a_rule_without_validators` |
| flex/flex.test.js > Test processBeforeValidate and processAfterValidate invocations > processPreAction works correctly, two whens with same events but different actions | `workflow::flex_tests::two_whens_with_the_same_event_prepare_each_rules_own_actions` |
| flex/flex.test.js > #executor > Bad YML | `upstream_filters_flex::flex_executor::bad_yml_creates_one_cancelled_check_run` |
| flex/flex.test.js > #executor > One When | `workflow::flex_tests::validators_are_registered_dynamically_and_unsupported_actions_are_skipped` |
| flex/flex.test.js > #executor > Comma seperated events | `workflow::flex_tests::comma_separated_whens_run_only_event_supporting_validators_and_actions` |
| flex/flex.test.js > #executor > Multiple Whens | `workflow::flex_tests::multiple_whens_run_only_the_rules_of_the_current_event` |
| flex/flex.test.js > #executor > isEventInContext is working only for correct event | `workflow::flex_tests::rules_run_only_for_the_event_in_the_context` |
| flex/flex.test.js > #executor > Error handling | `workflow::flex_tests::throwing_validator_runs_the_error_actions_but_not_the_pass_actions` |
| interceptors/checkReRun.test.js > (top level) > context is not modified if pre conditions are not met | `upstream_filters_flex::check_rerun_interceptor::context_changes_only_for_a_rerequested_check_run_with_matching_id_and_pull_requests` |
| interceptors/checkReRun.test.js > (top level) > #possibleInjection | `interceptors::tests::differing_check_run_and_metadata_ids_are_a_possible_injection` |
| interceptors/checkReRun.test.js > (top level) > #process | `upstream_filters_flex::check_rerun_interceptor::rerequested_check_run_becomes_the_event_stored_in_its_metadata` |
| interceptors/milestoned.test.js > (top level) > #valid | `interceptors::tests::only_milestone_actions_on_pull_request_issues_are_milestoned_pull_requests` |
| interceptors/milestoned.test.js > (top level) > #process | `upstream_filters_flex::milestoned_interceptor::milestoned_pull_request_issue_becomes_a_pull_request_event` |
| interceptors/push.test.js > push interceptor test > context is not modified if pre conditions are not met | `upstream_filters_flex::push_interceptor::context_is_not_modified_by_other_events_or_by_a_config_push` |
| interceptors/push.test.js > push interceptor test > call processWorkflow with modified context | `upstream_filters_flex::push_interceptor::config_push_runs_the_workflow_for_each_open_pull_request_as_push_synchronize` |
| interceptors/push.test.js > push interceptor test > do nothing if `head_commit` property is null | `upstream_filters_flex::push_interceptor::push_without_head_commit_does_nothing` |
| settings/settings.test.js > Loading bad settings > bad YML | `upstream_filters_flex::settings_loading::bad_yml_reports_one_bad_yml_error` |
| settings/settings.test.js > Loading bad settings > No YML | `upstream_filters_flex::settings_loading::missing_settings_file_yields_the_default_settings` |
| settings/settings.test.js > Loading bad settings > wrong version | `settings::tests::non_numeric_version_is_reported_as_unknown_version` |
| settings/settings.test.js > Loading bad settings > missing mergeable node | `settings::tests::settings_without_mergeable_node_report_missing_mergeable_node` |
| settings/settings.test.js > Loading bad settings > missing rule sets | `settings::tests::empty_mergeable_node_reports_missing_rule_sets` |
| settings/settings.test.js > settings file fetching > fetch the settings from first repo | `upstream_filters_flex::settings_loading::settings_are_fetched_from_the_organization_repository_first` |
| settings/settings.test.js > settings file fetching > check settings cache | `upstream_filters_flex::settings_loading::settings_cache_fetches_the_settings_file_only_once` |
| settings/settings.test.js > settings file fetching > check settings cache fetch | `upstream_filters_flex::settings_loading::cached_settings_are_used_without_fetching` |
| settings/settings.test.js > settings file fetching > check settings cache invalidated on push events | `upstream_filters_flex::settings_loading::push_adding_the_settings_file_invalidates_the_settings_cache` |
| metaData.test.js > (top level) > #deserialize | `metadata::tests::deserialize_reads_the_metadata_embedded_in_a_check_run_text` |
| metaData.test.js > (top level) > #serialize | `metadata::tests::serialize_wraps_the_metadata_in_an_html_comment_that_deserializes_back` |
| metaData.test.js > (top level) > #exists | `metadata::tests::exists_requires_both_metadata_markers` |
| mergeable.test.js > Mergeable > starting in dev mode and flexed correctly | `upstream_filters_flex::mergeable_start::development_mode_schedules_every_two_seconds_and_serves_webhooks` |
| mergeable.test.js > Mergeable > starting in production mode and flexed correctly | `upstream_filters_flex::mergeable_start::production_mode_schedules_every_hour_and_serves_webhooks` |

## Notes

- Jest spies on internals (`Teams.extractTeamMemberships`, `processWorkflow`, `octokit.*`) are replaced by stubbed GitHub
  API responses (team membership and members endpoints, topics, pulls, contents) or, for the flex executor, by counting
  validator/action fakes injected into the registry (`Registry::replace_validator`/`replace_action`, test-only).
- and/or/not: the upstream tests drive the sub-filters through the `repository` filter's topics, so the ports use the
  topics endpoint too (the older baseRef-based unit tests in `filters::logical_connective::tests` stay as extra coverage).
- Not > "Error is returned when filter has more than one item": the upstream body only checks a missing `filter` (marked
  TODO); the port checks the title's case (two sub-filters), the missing case is covered by the next row.
- push interceptor: the upstream mocks `processWorkflow`; the port loads a configuration whose only rule is
  `pull_request.push_synchronize` with a comment action, so the comment on the open pull request proves the workflow ran
  with event `pull_request`, action `push_synchronize` and that pull request.
- mergeable.test.js: the scheduler interval is checked on `AppConfig`; "flexed correctly" (webhook handling registered)
  is checked as the configured webhook route, the webhook flow itself is covered by the recorded contract cases.
- settings "check settings cache invalidated on push events": the cached copy is made stale so the test proves the
  refetch. Production fix: the settings cache is now invalidated by a push adding or modifying the settings file
  (upstream `Settings.resetCache`; it keyed on `context.event`, the port uses the event name `push`).
