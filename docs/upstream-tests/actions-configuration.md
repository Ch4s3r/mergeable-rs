# Upstream tests: actions and configuration

Upstream: `__tests__/unit/actions/` and `__tests__/unit/configuration/` of mergeability/mergeable.

- Upstream tests in area: 146 (each `test.each` row counted as one test)
- Ported: 146 (50 unit tests in `src/`, 96 integration tests in `tests/upstream_actions_configuration/mod.rs` against the mock GitHub API of `tests/contract/mock.rs`)
- Not applicable: 0

Rust test paths: `actions::…`, `annotations::…`, `configuration::…` are library unit tests (`src/`);
`contract_cases::upstream_actions_configuration::…` are integration tests run by `tests/contract_cases.rs`.
The `undefined` rows of `test.each` fall back to the mock context's default event, `pull_request`.

## Deviations from the JS test and production changes

- `configuration::fetch_config_file` ran the config cache reset as a detached task, so a push that changed the
  config file could still be answered from the stale cache. Upstream deletes the key synchronously before reading it.
  The reset is now awaited before the cache lookup (tests `config_cache_is_invalidated_on_push_events`,
  `config_cache_of_the_organization_is_invalidated_on_push_events`).
- `bad YML`, `that instanceWithContext return error if mergeable.yml is not found on PRs` and
  `that env USE_ORG_AS_DEFAULT_CONFIG correctly use org-wide config` depend on behaviour of the JS mocks; see the notes in the table.

| Upstream test (file > describe > title) | Rust test (path::name) or 'not applicable: reason' |
|---|---|
| actions/action.test.js > Action#isEventSupported > Returns correctly with one supported event | `actions::tests::is_event_supported_returns_correctly_with_one_supported_event` |
| actions/action.test.js > Action#isEventSupported > Returns correctly with more than one supported event | `actions::tests::is_event_supported_returns_correctly_with_more_than_one_supported_event` |
| actions/action.test.js > Action#isEventSupported > Returns correctly with a wildcard in the event name | `actions::tests::is_event_supported_returns_correctly_with_a_wildcard_in_the_event_name` |
| actions/action.test.js > Action#getActionables > Returns an item when there is no validation defined | `actions::tests::actionables_return_an_item_when_there_is_no_validation_defined` |
| actions/action.test.js > Action#getActionables > Returns correct items when there is validation and the event is schedule | `actions::tests::actionables_return_correct_items_when_there_is_validation_and_the_event_is_schedule` |
| actions/action.test.js > Action#getActionables > when event is not schedule | `actions::tests::actionables_return_the_event_item_when_event_is_not_schedule` |
| actions/action.test.js > Action#getEventAttributes > Extracts event properties from pull_request correctly | `actions::tests::event_attributes_are_extracted_from_pull_request_correctly` |
| actions/action.test.js > Action#getEventAttributes > Extracts event properties from issues correctly | `actions::tests::event_attributes_are_extracted_from_issues_correctly` |
| actions/action.test.js > Action#getEventAttributes > Defaults event properties on schedule event | `actions::tests::event_attributes_default_on_schedule_event` |
| actions/lib/searchAndReplaceSpecialAnnotation.test.js > searchAndReplaceSpecialAnnotations > does not affect input if no special annotations are found | `annotations::tests::does_not_affect_input_if_no_special_annotations_are_found` |
| actions/lib/searchAndReplaceSpecialAnnotation.test.js > searchAndReplaceSpecialAnnotations > special annotation at the beginning of string works properly | `annotations::tests::special_annotation_at_the_beginning_of_string_works_properly` |
| actions/lib/searchAndReplaceSpecialAnnotation.test.js > searchAndReplaceSpecialAnnotations > escape character works properly | `annotations::tests::escape_character_works_properly` |
| actions/lib/searchAndReplaceSpecialAnnotation.test.js > searchAndReplaceSpecialAnnotations > special annotation at the end of string works properly | `annotations::tests::special_annotation_at_the_end_of_string_works_properly` |
| actions/lib/searchAndReplaceSpecialAnnotation.test.js > searchAndReplaceSpecialAnnotations > @@annotation is replaced, prepending @ remains | `annotations::tests::double_at_annotation_is_replaced_and_prepending_at_remains` |
| actions/lib/searchAndReplaceSpecialAnnotation.test.js > searchAndReplaceSpecialAnnotations > replaces special annotation anywhere in the text | `annotations::tests::replaces_special_annotation_anywhere_in_the_text` |
| actions/assign.test.js > check that assign is called for undefined events | `contract_cases::upstream_actions_configuration::assign_is_called_for_default_pull_request_events` |
| actions/assign.test.js > check that assign is called for pull_request events | `contract_cases::upstream_actions_configuration::assign_is_called_for_pull_request_events` |
| actions/assign.test.js > check that assign is called for issues events | `contract_cases::upstream_actions_configuration::assign_is_called_for_issues_events` |
| actions/assign.test.js > check that assign is called for issue_comment events | `contract_cases::upstream_actions_configuration::assign_is_called_for_issue_comment_events` |
| actions/assign.test.js > check that assignees are added when afterValidate is called with proper parameter | `contract_cases::upstream_actions_configuration::assignees_are_added_when_after_validate_is_called_with_proper_parameter` |
| actions/assign.test.js > check that creator is added when assignee is @author or @sender or @bot | `contract_cases::upstream_actions_configuration::creator_is_added_when_assignee_is_author_sender_or_bot` |
| actions/assign.test.js > check only authorized users are added as assignee | `contract_cases::upstream_actions_configuration::only_authorized_users_are_added_as_assignee` |
| actions/close.test.js > check that close is called for undefined events | `contract_cases::upstream_actions_configuration::close_is_called_for_default_pull_request_events` |
| actions/close.test.js > check that close is called for pull_request events | `contract_cases::upstream_actions_configuration::close_is_called_for_pull_request_events` |
| actions/close.test.js > check that close is called for issues events | `contract_cases::upstream_actions_configuration::close_is_called_for_issues_events` |
| actions/close.test.js > check that close is called for issue_comment events | `contract_cases::upstream_actions_configuration::close_is_called_for_issue_comment_events` |
| actions/close.test.js > check that close is called for schedule events | `contract_cases::upstream_actions_configuration::close_is_called_for_schedule_events` |
| actions/close.test.js > check that issue is closed | `contract_cases::upstream_actions_configuration::issue_is_closed` |
| actions/close.test.js > check that issues from scheduler are closed | `contract_cases::upstream_actions_configuration::issues_from_scheduler_are_closed` |
| actions/request_review.test.js > check that user is requested a review if user is an collaborator | `contract_cases::upstream_actions_configuration::user_is_requested_a_review_if_user_is_a_collaborator` |
| actions/request_review.test.js > check that author is removed from list of reviewer to request | `contract_cases::upstream_actions_configuration::author_is_removed_from_list_of_reviewers_to_request` |
| actions/request_review.test.js > that requested Reviewers are not requested again | `contract_cases::upstream_actions_configuration::requested_reviewers_are_not_requested_again` |
| actions/request_review.test.js > that non collaborator is not requested reviews | `contract_cases::upstream_actions_configuration::non_collaborator_is_not_requested_reviews` |
| actions/request_review.test.js > that requested teams are not requested again | `contract_cases::upstream_actions_configuration::requested_teams_are_not_requested_again` |
| actions/request_review.test.js > that requested team reviewers are added | `contract_cases::upstream_actions_configuration::requested_team_reviewers_are_added` |
| actions/checks.test.js > that checks is called for undefined events | `contract_cases::upstream_actions_configuration::checks_is_called_for_default_pull_request_events` |
| actions/checks.test.js > that checks is called for pull_request events | `contract_cases::upstream_actions_configuration::checks_is_called_for_pull_request_events` |
| actions/checks.test.js > that checks is called for pull_request_review events | `contract_cases::upstream_actions_configuration::checks_is_called_for_pull_request_review_events` |
| actions/checks.test.js > that checks is called for issue_comment events | `contract_cases::upstream_actions_configuration::checks_is_called_for_issue_comment_events` |
| actions/checks.test.js > that run calls create api | `contract_cases::upstream_actions_configuration::run_calls_create_api` |
| actions/checks.test.js > that checks created when doPostAction is called with proper parameter | `contract_cases::upstream_actions_configuration::checks_are_created_when_before_validate_is_called_with_proper_parameter` |
| actions/checks.test.js > that beforeValidate stores the name correctly | `contract_cases::upstream_actions_configuration::before_validate_stores_the_name_correctly` |
| actions/checks.test.js > that `conclusion` and `completed_at` fields are not set when `settings.state` is not `completed` | `contract_cases::upstream_actions_configuration::conclusion_and_completed_at_are_not_set_when_state_is_not_completed` |
| actions/checks.test.js > that afterValidate is called with properly and output is correct | `contract_cases::upstream_actions_configuration::after_validate_output_is_correct` |
| actions/checks.test.js > that afterValidate is replacing special annotations in payload | `contract_cases::upstream_actions_configuration::after_validate_replaces_special_annotations_in_payload` |
| actions/checks.test.js > that afterValidate is correct when validation fails | `contract_cases::upstream_actions_configuration::after_validate_is_correct_when_validation_fails` |
| actions/checks.test.js > that correct name is used afterValidate payload | `contract_cases::upstream_actions_configuration::correct_name_is_used_in_after_validate_payload` |
| actions/merge.test.js > check that merge is called for status events | `contract_cases::upstream_actions_configuration::merge_is_called_for_status_events` |
| actions/merge.test.js > check that merge is called for undefined events | `contract_cases::upstream_actions_configuration::merge_is_called_for_default_pull_request_events` |
| actions/merge.test.js > check that merge is called for pull_request_review events | `contract_cases::upstream_actions_configuration::merge_is_called_for_pull_request_review_events` |
| actions/merge.test.js > check that merge is called for check_suite events | `contract_cases::upstream_actions_configuration::merge_is_called_for_check_suite_events` |
| actions/merge.test.js > check that merge is called for issue_comment events | `contract_cases::upstream_actions_configuration::merge_is_called_for_issue_comment_events` |
| actions/merge.test.js > check that merge is not called if PR is in a blocked state | `contract_cases::upstream_actions_configuration::merge_is_not_called_if_pull_request_is_in_a_blocked_state` |
| actions/merge.test.js > check that merge is not called if PR is closed | `contract_cases::upstream_actions_configuration::merge_is_not_called_if_pull_request_is_closed` |
| actions/merge.test.js > check that merge is not called if PR has not been merged | `contract_cases::upstream_actions_configuration::merge_is_not_called_if_pull_request_is_already_merged` |
| actions/merge.test.js > check that merge_method option works correctly | `contract_cases::upstream_actions_configuration::merge_method_option_works_correctly` |
| actions/merge.test.js > check that commit_title and commit_message options work correctly | `contract_cases::upstream_actions_configuration::commit_title_and_commit_message_options_work_correctly` |
| actions/merge.test.js > check that commit_title and commit_message options support templating | `contract_cases::upstream_actions_configuration::commit_title_and_commit_message_options_support_templating` |
| actions/merge.test.js > check that commit_title and commit_message options support empty string | `contract_cases::upstream_actions_configuration::commit_title_and_commit_message_options_support_empty_string` |
| actions/merge.test.js > check that commit_title and commit_message options do not escape html | `contract_cases::upstream_actions_configuration::commit_title_and_commit_message_options_do_not_escape_html` |
| actions/labels.test.js > check that close is called for undefined events | `contract_cases::upstream_actions_configuration::labels_are_set_for_default_pull_request_events` |
| actions/labels.test.js > check that close is called for pull_request events | `contract_cases::upstream_actions_configuration::labels_are_set_for_pull_request_events` |
| actions/labels.test.js > check that close is called for issues events | `contract_cases::upstream_actions_configuration::labels_are_set_for_issues_events` |
| actions/labels.test.js > check that close is called for issue_comment events | `contract_cases::upstream_actions_configuration::labels_are_set_for_issue_comment_events` |
| actions/labels.test.js > check that close is called for schedule events | `contract_cases::upstream_actions_configuration::labels_are_set_for_schedule_events` |
| actions/labels.test.js > check that replace replaces existing labels | `contract_cases::upstream_actions_configuration::replace_replaces_existing_labels` |
| actions/labels.test.js > check that add appends to existing labels | `contract_cases::upstream_actions_configuration::add_appends_to_existing_labels` |
| actions/labels.test.js > check that delete removes from existing labels | `contract_cases::upstream_actions_configuration::delete_removes_from_existing_labels` |
| actions/labels.test.js > check the order of replace, add, delete | `contract_cases::upstream_actions_configuration::label_operations_run_in_order_replace_add_delete` |
| actions/labels.test.js > check that settings can be a single value | `contract_cases::upstream_actions_configuration::label_operation_settings_can_be_a_single_value` |
| actions/labels.test.js > check that unknown mode throw errors | `contract_cases::upstream_actions_configuration::unknown_label_mode_throws_an_unsupported_setting_error` |
| actions/labels.test.js > check that labels setting accepts an array | `contract_cases::upstream_actions_configuration::labels_setting_accepts_an_array` |
| actions/labels.test.js > check that labels setting defaults to add mode | `contract_cases::upstream_actions_configuration::labels_setting_defaults_to_add_mode` |
| actions/labels.test.js > check that set mode works | `contract_cases::upstream_actions_configuration::replace_mode_sets_the_labels` |
| actions/labels.test.js > check that delete mode works | `contract_cases::upstream_actions_configuration::delete_mode_removes_labels_matching_globs` |
| actions/labels.test.js > check that issues from scheduler are labelled | `contract_cases::upstream_actions_configuration::issues_from_scheduler_are_labelled` |
| actions/labels.test.js > check that labels from scheduler are deleted | `contract_cases::upstream_actions_configuration::labels_from_scheduler_are_deleted` |
| actions/labels.test.js > check that labels from scheduler are set | `contract_cases::upstream_actions_configuration::labels_from_scheduler_are_set` |
| actions/comment.test.js > check that comment is called for undefined events | `contract_cases::upstream_actions_configuration::comment_is_called_for_default_pull_request_events` |
| actions/comment.test.js > check that comment is called for pull_request events | `contract_cases::upstream_actions_configuration::comment_is_called_for_pull_request_events` |
| actions/comment.test.js > check that comment is called for issues events | `contract_cases::upstream_actions_configuration::comment_is_called_for_issues_events` |
| actions/comment.test.js > check that comment is called for issue_comment events | `contract_cases::upstream_actions_configuration::comment_is_called_for_issue_comment_events` |
| actions/comment.test.js > check that comment is called for schedule events | `contract_cases::upstream_actions_configuration::comment_is_called_for_schedule_events` |
| actions/comment.test.js > check that comment created when afterValidate is called with proper parameter | `contract_cases::upstream_actions_configuration::comment_is_created_when_after_validate_is_called_with_proper_parameter` |
| actions/comment.test.js > that comment is created three times when result contain three issues found to be acted on | `contract_cases::upstream_actions_configuration::comment_is_created_three_times_when_result_contains_three_issues_to_act_on` |
| actions/comment.test.js > check that old comments from Mergeable are deleted if they exists | `contract_cases::upstream_actions_configuration::old_comments_from_mergeable_are_deleted_if_they_exist` |
| actions/comment.test.js > check that old comments checks toLowerCase of the Bot name | `contract_cases::upstream_actions_configuration::old_comments_check_the_lower_cased_bot_name` |
| actions/comment.test.js > error handling includes removing old error comments and creating new error comment | `contract_cases::upstream_actions_configuration::error_handling_removes_old_error_comments_and_creates_a_new_error_comment` |
| actions/comment.test.js > remove error comments only remove comments that includes "error" | `contract_cases::upstream_actions_configuration::remove_error_comments_only_removes_comments_that_include_error` |
| actions/comment.test.js > check that leave_old_comment option works | `contract_cases::upstream_actions_configuration::leave_old_comment_option_keeps_old_comments` |
| actions/comment.test.js > remove Error comment fail gracefully if payload does not exists | `contract_cases::upstream_actions_configuration::remove_error_comments_fails_gracefully_if_payload_does_not_exist` |
| actions/comment.test.js > special annotations are replaced | `contract_cases::upstream_actions_configuration::special_annotations_are_replaced_in_comments` |
| configuration/configuration.test.js > Loading bad configuration > bad YML | `contract_cases::upstream_actions_configuration::bad_yml_in_the_configuration_is_reported_as_bad_yml`<br>Note: The JS test makes the mocked `probotContext.config` throw a `YAMLException`; the Rust test serves invalid YAML as the config changed in the pull request, the path where mergeable itself parses YAML (as in recorded case `config-invalid-yaml-in-pr`). |
| configuration/configuration.test.js > Loading bad configuration > No YML found | `contract_cases::upstream_actions_configuration::missing_yml_is_reported_as_no_yml` |
| configuration/configuration.test.js > Loading bad configuration > wrong version | `configuration::tests::wrong_version_is_reported_as_unknown_version` |
| configuration/configuration.test.js > Loading bad configuration > missing mergeable node | `configuration::tests::missing_mergeable_node_is_reported` |
| configuration/configuration.test.js > Loading bad configuration > missing rule sets | `configuration::tests::missing_rule_sets_are_reported` |
| configuration/configuration.test.js > Loading bad configuration > v2: non array rule set | `configuration::tests::v2_non_array_rule_set_is_reported` |
| configuration/configuration.test.js > Loading bad configuration > v2: missing/typo in "validate" keyword multiple rule sets | `configuration::tests::v2_missing_or_misspelled_validate_keyword_is_reported_for_multiple_rule_sets` |
| configuration/configuration.test.js > Loading bad configuration > v2: non-array "validate" node | `configuration::tests::v2_non_array_validate_node_is_reported` |
| configuration/configuration.test.js > Loading bad configuration > v2: missing/typo in "when" keyword rule sets | `configuration::tests::v2_missing_or_misspelled_when_keyword_is_reported` |
| configuration/configuration.test.js > Loading bad configuration > multiple errors | `configuration::tests::multiple_errors_are_all_reported` |
| configuration/configuration.test.js > config file fetching > fetch from main branch if event is not pull_request | `contract_cases::upstream_actions_configuration::config_is_fetched_from_main_branch_if_event_is_not_pull_request` |
| configuration/configuration.test.js > config file fetching > check config cache | `contract_cases::upstream_actions_configuration::config_cache_is_warmed_and_then_used` |
| configuration/configuration.test.js > config file fetching > check config cache fetch | `contract_cases::upstream_actions_configuration::config_cache_is_used_when_already_filled` |
| configuration/configuration.test.js > config file fetching > check config cache invalidated on push events | `contract_cases::upstream_actions_configuration::config_cache_is_invalidated_on_push_events`<br>Note: Failed before the production fix (cache reset ran detached, so the stale cached config was returned). |
| configuration/configuration.test.js > config file fetching > check config cache for org invalidated on push events | `contract_cases::upstream_actions_configuration::config_cache_of_the_organization_is_invalidated_on_push_events`<br>Note: Failed before the production fix, see above. |
| configuration/configuration.test.js > config file fetching > fetch from main branch if the event is PR relevant and file is not modified or added | `contract_cases::upstream_actions_configuration::config_is_fetched_from_main_branch_if_the_pull_request_does_not_change_it` |
| configuration/configuration.test.js > config file fetching > fetch from main branch if the event is PR relevant and PR is from a fork | `contract_cases::upstream_actions_configuration::config_is_fetched_from_main_branch_if_the_pull_request_is_from_a_fork` |
| configuration/configuration.test.js > config file fetching > fetch from head branch if the event is relevant to PR and file is modified or added | `contract_cases::upstream_actions_configuration::config_is_fetched_from_head_branch_if_the_pull_request_modifies_or_adds_it` |
| configuration/configuration.test.js > config file fetching > fetch from head branch if the event is relevant to PR and file is modified or added and cache is enabled | `contract_cases::upstream_actions_configuration::config_is_fetched_from_head_branch_if_the_pull_request_modifies_or_adds_it_with_cache_enabled` |
| configuration/configuration.test.js > with version 2 > it loads correctly without version | `configuration::tests::version_two_shape_loads_correctly_without_version` |
| configuration/configuration.test.js > with version 1 > that constructor loads settings correctly | `configuration::tests::version_one_constructor_loads_settings_correctly` |
| configuration/configuration.test.js > with version 1 > that defaults are not injected when user defined configuration exists | `configuration::tests::version_one_defaults_are_not_injected_when_user_defined_configuration_exists` |
| configuration/configuration.test.js > with version 1 > that instanceWithContext returns the right Configuration | `contract_cases::upstream_actions_configuration::loading_with_context_returns_the_right_configuration` |
| configuration/configuration.test.js > with version 1 > that instanceWithContext returns the right Configuration (pull_requests) | `contract_cases::upstream_actions_configuration::loading_with_context_returns_the_right_configuration_for_pull_requests` |
| configuration/configuration.test.js > with version 1 > that instanceWithContext returns the right Configuration (issues) | `contract_cases::upstream_actions_configuration::loading_with_context_returns_the_right_configuration_for_issues` |
| configuration/configuration.test.js > with version 1 > that instanceWithContext loads the configuration for stale correctly when specified for pull_requests and issues separately | `contract_cases::upstream_actions_configuration::loading_with_context_loads_stale_separately_for_pull_requests_and_issues` |
| configuration/configuration.test.js > with version 1 > that instanceWithContext return error if mergeable.yml is not found | `contract_cases::upstream_actions_configuration::loading_with_context_returns_no_yml_error_if_mergeable_yml_is_not_found` |
| configuration/configuration.test.js > with version 1 > that instanceWithContext return error if mergeable.yml is not found on PRs | `contract_cases::upstream_actions_configuration::loading_with_context_returns_github_api_error_if_the_pull_request_config_cannot_be_fetched`<br>Note: The JS mock rejects with an `HttpError` whose `status` is the string `Not Found`, so mergeable does not treat it as a 404 and reports GITHUB_API_ERROR. A real 404 is treated as "no file" in both implementations, so the Rust test answers 403 to get the same non-404 API failure. |
| configuration/configuration.test.js > with version 1 > that if pass, fail or error is undefined in v2 config, the config will not break | `configuration::tests::undefined_pass_fail_or_error_does_not_break_the_config` |
| configuration/configuration.test.js > with version 1 > that fetchConfigFile returns the right Configuration depending on USE_CONFIG_FROM_PULL_REQUEST env | `contract_cases::upstream_actions_configuration::use_config_from_pull_request_setting_selects_the_config_source` |
| configuration/configuration.test.js > with version 1 > that env USE_ORG_AS_DEFAULT_CONFIG correctly use org-wide config | `contract_cases::upstream_actions_configuration::use_org_as_default_config_setting_merges_the_organization_wide_config`<br>Note: Asserts organization rules before repository rules. The JS test expects the opposite order only because its `probotContext.config` mock merges `[config, defaultConfig]`; probot 13 merges `[defaultConfig, ...configs]`, which is what the Rust code does. No recorded case covers this order. |
| configuration/transformers/v1Config.test.js > Description no empty scenarios transformations > no_empty using simple config | `configuration::v1_config::tests::description_no_empty_using_simple_config` |
| configuration/transformers/v1Config.test.js > Description no empty scenarios transformations > no_empty using advanced config | `configuration::v1_config::tests::description_no_empty_using_advanced_config` |
| configuration/transformers/v1Config.test.js > Description no empty scenarios transformations > no-empty using simple config | `configuration::v1_config::tests::description_no_dash_empty_using_simple_config` |
| configuration/transformers/v1Config.test.js > Description no empty scenarios transformations > no-empty using advanced config | `configuration::v1_config::tests::description_no_dash_empty_using_advanced_config` |
| configuration/transformers/v1Config.test.js > Stale scenarios > No stale configuration | `configuration::v1_config::tests::stale_absent_creates_no_scheduled_rule_set` |
| configuration/transformers/v1Config.test.js > Stale scenarios > Configuration in pulls only. | `configuration::v1_config::tests::stale_configuration_in_pulls_only` |
| configuration/transformers/v1Config.test.js > Stale scenarios > Configuration in issues only | `configuration::v1_config::tests::stale_configuration_in_issues_only` |
| configuration/transformers/v1Config.test.js > Stale scenarios > Configuration in both pull and issues | `configuration::v1_config::tests::stale_configuration_in_both_pulls_and_issues` |
| configuration/transformers/v1Config.test.js > check that proper format is returned, including default pass, fail and error | `configuration::v1_config::tests::proper_format_is_returned_including_default_pass_fail_and_error` |
| configuration/transformers/v1Config.test.js > checks that the content is tranformed correctly | `configuration::v1_config::tests::content_is_transformed_correctly` |
| configuration/transformers/v1Config.test.js > check all the simple config is transformed accurately | `configuration::v1_config::tests::all_the_simple_config_is_transformed_accurately` |
| configuration/transformers/v1Config.test.js > checks all advanced config is transformed accurately | `configuration::v1_config::tests::all_advanced_config_is_transformed_accurately` |
| configuration/transformers/v1Config.test.js > check that and/or logic is transformed correctly | `configuration::v1_config::tests::and_or_logic_is_transformed_correctly` |
| configuration/transformers/v2Config.test.js > pass, fail, error defaults will load when pull_request event is specified. | `configuration::tests::outcome_defaults_load_when_pull_request_event_is_specified` |
| configuration/transformers/v2Config.test.js > pass, fail, error defaults will load when pull_request event is specified and user specified configs are not overridden. | `configuration::tests::outcome_defaults_load_for_pull_requests_without_overriding_user_specified_outcomes` |
| configuration/transformers/v2Config.test.js > pass, fail, error defaults will load when pull_request is mixed with other events. | `configuration::tests::outcome_defaults_load_when_pull_request_is_mixed_with_other_events` |
| configuration/transformers/v2Config.test.js > pass, fail, error defaults will load when issue_comment event is specified. | `configuration::tests::outcome_defaults_load_when_issue_comment_event_is_specified` |
| configuration/transformers/v2Config.test.js > pass, fail, error defaults will load when pull_request_review event is specified. | `configuration::tests::outcome_defaults_load_when_pull_request_review_event_is_specified` |
| configuration/transformers/v2Config.test.js > only pass, fail defaults ignore recipes that are not for pull_requests | `configuration::tests::outcome_defaults_ignore_recipes_that_are_not_for_pull_requests` |
| configuration/transformers/v2Config.test.js > default checks fill in missing required fields | `configuration::tests::default_checks_fill_in_missing_required_fields` |
| configuration/transformers/v2Config.test.js > adding default only works for checks | `configuration::tests::adding_defaults_only_works_for_checks` |
| configuration/transformers/v2Config.test.js > defaults are not added to all cases if no checks exists | `configuration::tests::defaults_are_not_added_to_all_outcomes_if_no_checks_exist` |
| configuration/transformers/v2Config.test.js > defaults are not added to all cases if at least one checks exists | `configuration::tests::defaults_are_added_to_all_outcomes_if_at_least_one_checks_exists` |
