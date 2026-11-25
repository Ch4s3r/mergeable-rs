# Upstream validator tests a–l

Upstream: `__tests__/unit/validators/` files `a*`–`l*` and `options_processor/**` of mergeability/mergeable.

- Upstream tests: 237
- Ported: 236 (171 integration tests in `tests/upstream_validators_a_l` against the mock GitHub API, 65 unit tests in `src`, 4 of them pre-existing)
- Not applicable: 1

Validator tests run `validators::process_validate` on a context whose GitHub client points at the mock GitHub API
(`tests/upstream_validators_a_l/mock_context.rs` mirrors `__fixtures__/unit/helper.js`). Where the JS test mocks an
internal module (`Teams`, `Owners`), the Rust test stubs the GitHub endpoints that module calls instead.

| Upstream test (file > describe > title) | Rust test (path::name) or 'not applicable: reason' |
|---|---|
| age.test.js > age validator > checks that days is validated properly | `tests/upstream_validators_a_l/age.rs::checks_that_days_is_validated_properly` |
| age.test.js > age validator > that updated_at option is working | `tests/upstream_validators_a_l/age.rs::that_updated_at_option_is_working` |
| age.test.js > age validator > that message option works properly | `tests/upstream_validators_a_l/age.rs::that_message_option_works_properly` |
| and.test.js > And Validator Unit Test > should run subtasks | `tests/upstream_validators_a_l/and.rs::should_run_subtasks` |
| and.test.js > And Validator Unit Test > should return output of second task if first fails | `tests/upstream_validators_a_l/and.rs::should_return_output_of_second_task_if_first_fails` |
| and.test.js > And Validator Unit Test > should return output of first task to pass when multiple are given | `tests/upstream_validators_a_l/and.rs::should_return_output_of_first_task_to_pass_when_multiple_are_given` |
| and.test.js > And Validator Unit Test > Error is returned when validate is missing | `tests/upstream_validators_a_l/and.rs::error_is_returned_when_validate_is_missing` |
| and.test.js > And Validator Unit Test > Error is returned when validate is not an array | `tests/upstream_validators_a_l/and.rs::error_is_returned_when_validate_is_not_an_array` |
| and.test.js > And Validator Unit Test > Error is returned when validate is empty | `tests/upstream_validators_a_l/and.rs::error_is_returned_when_validate_is_empty` |
| and.test.js > And Validator Unit Test > Error is returned when validate uses unsupported classes | `tests/upstream_validators_a_l/and.rs::error_is_returned_when_validate_uses_unsupported_classes` |
| and.test.js > And Validator Unit Test > Supports nested and validator | `tests/upstream_validators_a_l/and.rs::supports_nested_and_validator` |
| and.test.js > And Validator Unit Test > error if one of the sub validator errored | `tests/upstream_validators_a_l/and.rs::error_if_one_of_the_sub_validator_errored` |
| approvals.test.js > that approvals work with no owners file | `tests/upstream_validators_a_l/approvals.rs::that_approvals_work_with_no_owners_file` |
| approvals.test.js > that mergeable is false when less than minimum | `tests/upstream_validators_a_l/approvals.rs::that_mergeable_is_false_when_less_than_minimum` |
| approvals.test.js > that mergeable is true when the same as minimum | `tests/upstream_validators_a_l/approvals.rs::that_mergeable_is_true_when_the_same_as_minimum` |
| approvals.test.js > that mergeable is true when greater than minimum | `tests/upstream_validators_a_l/approvals.rs::that_mergeable_is_true_when_greater_than_minimum` |
| approvals.test.js > that description is dynamic based on minimum | `tests/upstream_validators_a_l/approvals.rs::that_description_is_dynamic_based_on_minimum` |
| approvals.test.js > that description is null when mergeable | `tests/upstream_validators_a_l/approvals.rs::that_description_is_null_when_mergeable` |
| approvals.test.js > mergeable is false if required member(s) has not approved | `tests/upstream_validators_a_l/approvals.rs::mergeable_is_false_if_required_members_have_not_approved` |
| approvals.test.js > mergeable is false if excluded member(s) has approved | `tests/upstream_validators_a_l/approvals.rs::mergeable_is_false_if_excluded_members_have_approved` |
| approvals.test.js > mergeable passes if required user has approved | `tests/upstream_validators_a_l/approvals.rs::mergeable_passes_if_required_user_has_approved` |
| approvals.test.js > validate correctly when one required user approved but followed up with comments. | `tests/upstream_validators_a_l/approvals.rs::validate_correctly_when_one_required_user_approved_but_followed_up_with_comments` |
| approvals.test.js > validate correctly when one required user approved but followed up with REQUEST_CHANGE or PENDING. | `tests/upstream_validators_a_l/approvals.rs::validate_correctly_when_one_required_user_approved_but_followed_up_with_request_change_or_pending` |
| approvals.test.js > validate correctly with reviews more than 30. | `tests/upstream_validators_a_l/approvals.rs::validate_correctly_with_reviews_more_than_30` |
| approvals.test.js > pr creator is removed from required reviewer list | `tests/upstream_validators_a_l/approvals.rs::pr_creator_is_removed_from_required_reviewer_list` |
| approvals.test.js > checks that latest review is used | `tests/upstream_validators_a_l/approvals.rs::checks_that_latest_review_is_used` |
| approvals.test.js > mergeable advanceSetting min works | `tests/upstream_validators_a_l/approvals.rs::mergeable_advance_setting_min_works` |
| approvals.test.js > mergeable advanceSetting max works | `tests/upstream_validators_a_l/approvals.rs::mergeable_advance_setting_max_works` |
| approvals.test.js > required.owners  > that ownersEnabled will check owner file and append it to required list | `tests/upstream_validators_a_l/approvals.rs::required_owners_that_owners_enabled_will_check_owner_file_and_append_it_to_required_list` |
| approvals.test.js > required.owners  > that ownersEnabled will check owner file and is mergeable when approval is given | `tests/upstream_validators_a_l/approvals.rs::required_owners_that_owners_enabled_will_check_owner_file_and_is_mergeable_when_approval_is_given` |
| approvals.test.js > required.owners  > that ownersDisabled will call NOT call owners.process | `tests/upstream_validators_a_l/approvals.rs::required_owners_that_owners_disabled_will_not_call_owners_process` |
| approvals.test.js > that assignees are not required when assignees is not specified | `tests/upstream_validators_a_l/approvals.rs::that_assignees_are_not_required_when_assignees_is_not_specified` |
| approvals.test.js > that assignees are not required when assignees is false | `tests/upstream_validators_a_l/approvals.rs::that_assignees_are_not_required_when_assignees_is_false` |
| approvals.test.js > that assignees are required when assignees is true | `tests/upstream_validators_a_l/approvals.rs::that_assignees_are_required_when_assignees_is_true` |
| approvals.test.js > that assignees appends to the reviewers list correctly | `tests/upstream_validators_a_l/approvals.rs::that_assignees_appends_to_the_reviewers_list_correctly` |
| approvals.test.js > that requestedReviewers appends to the reviewers list correctly | `tests/upstream_validators_a_l/approvals.rs::that_requested_reviewers_appends_to_the_reviewers_list_correctly` |
| approvals.test.js > blocks if changes are requested | `tests/upstream_validators_a_l/approvals.rs::blocks_if_changes_are_requested` |
| approvals.test.js > blocks if changes are requested while other options area also passed in | `tests/upstream_validators_a_l/approvals.rs::blocks_if_changes_are_requested_while_other_options_are_also_passed_in` |
| approvals.test.js > returns proper error when team provided is not found | `tests/upstream_validators_a_l/approvals.rs::returns_proper_error_when_team_provided_is_not_found` (upstream mocks `Teams.extractTeamMemberships` to return the non-approving `userB`, which the membership lookup of approving reviewers cannot return; with no approver in the team the limit is empty and lifted, ADR-014) |
| approvals.test.js > required.owners  > limit.teams option works properly | `tests/upstream_validators_a_l/approvals.rs::required_owners_limit_teams_option_works_properly` |
| approvals.test.js > required.owners  > limit.users option works properly | `tests/upstream_validators_a_l/approvals.rs::required_owners_limit_users_option_works_properly` |
| approvals.test.js > required.owners  > limit.owners option works properly | `tests/upstream_validators_a_l/approvals.rs::required_owners_limit_owners_option_works_properly` |
| approvals.test.js > required.owners  > limit option will create an "info" validation | `tests/upstream_validators_a_l/approvals.rs::required_owners_limit_option_will_create_an_info_validation` |
| approvals.test.js > required.owners  > limit.owners passes when CODEOWNERS file is missing | `tests/upstream_validators_a_l/approvals.rs::required_owners_limit_owners_passes_when_codeowners_file_is_missing` |
| approvals.test.js > required.owners  > limit.owners passes when codeowner approved and teams are empty | `tests/upstream_validators_a_l/approvals.rs::required_owners_limit_owners_passes_when_codeowner_approved_and_teams_are_empty` |
| approvals.test.js > required.owners  > required.owners passes when CODEOWNERS file is missing and other reviewers have approved | `tests/upstream_validators_a_l/approvals.rs::required_owners_required_owners_passes_when_codeowners_file_is_missing_and_other_reviewers_have_approved` |
| assignee.test.js > that mergeable is false when less than minimum | `tests/upstream_validators_a_l/assignee.rs::that_mergeable_is_false_when_less_than_minimum` |
| assignee.test.js > that mergeable is true when the same as minimum | `tests/upstream_validators_a_l/assignee.rs::that_mergeable_is_true_when_the_same_as_minimum` |
| assignee.test.js > that mergeable is true when greater than minimum | `tests/upstream_validators_a_l/assignee.rs::that_mergeable_is_true_when_greater_than_minimum` |
| assignee.test.js > that description is dynamic based on minimum | `tests/upstream_validators_a_l/assignee.rs::that_description_is_dynamic_based_on_minimum` |
| assignee.test.js > that description is correct when mergeable | `tests/upstream_validators_a_l/assignee.rs::that_description_is_correct_when_mergeable` |
| assignee.test.js > checks that max is working | `tests/upstream_validators_a_l/assignee.rs::checks_that_max_is_working` |
| assignee.test.js > checks that advance_setting message is working | `tests/upstream_validators_a_l/assignee.rs::checks_that_advance_setting_message_is_working` |
| author.test.js > should fail with unexpected author | `tests/upstream_validators_a_l/author.rs::should_fail_with_unexpected_author` |
| author.test.js > should fail with unexpected one_of author | `tests/upstream_validators_a_l/author.rs::should_fail_with_unexpected_one_of_author` |
| author.test.js > should pass with expected author | `tests/upstream_validators_a_l/author.rs::should_pass_with_expected_author` |
| author.test.js > should pass with one_of author | `tests/upstream_validators_a_l/author.rs::should_pass_with_one_of_author` |
| author.test.js > should fail with excluded author | `tests/upstream_validators_a_l/author.rs::should_fail_with_excluded_author` |
| author.test.js > should fail with none_of author | `tests/upstream_validators_a_l/author.rs::should_fail_with_none_of_author` |
| author.test.js > should pass with excluded author | `tests/upstream_validators_a_l/author.rs::should_pass_with_excluded_author` |
| author.test.js > should pass with none_of author | `tests/upstream_validators_a_l/author.rs::should_pass_with_none_of_author` |
| author.test.js > should pass with expected author from correct team | `tests/upstream_validators_a_l/author.rs::should_pass_with_expected_author_from_correct_team` |
| author.test.js > should pass with one_of author from correct team | `tests/upstream_validators_a_l/author.rs::should_pass_with_one_of_author_from_correct_team` |
| author.test.js > should fail with expected author from incorrect team | `tests/upstream_validators_a_l/author.rs::should_fail_with_expected_author_from_incorrect_team` |
| author.test.js > should fail with one_of author from incorrect team | `tests/upstream_validators_a_l/author.rs::should_fail_with_one_of_author_from_incorrect_team` |
| author.test.js > should fail with unexpected author from correct team | `tests/upstream_validators_a_l/author.rs::should_fail_with_unexpected_author_from_correct_team` |
| author.test.js > should fail with one_of author from correct team | `tests/upstream_validators_a_l/author.rs::should_fail_with_one_of_author_from_correct_team` |
| author.test.js > should pass when the author is a member of the team | `tests/upstream_validators_a_l/author.rs::should_pass_when_the_author_is_a_member_of_the_team` |
| author.test.js > should pass when the author is one_of the members of the team | `tests/upstream_validators_a_l/author.rs::should_pass_when_the_author_is_one_of_the_members_of_the_team` |
| author.test.js > should fail when the author is not a member of the team | `tests/upstream_validators_a_l/author.rs::should_fail_when_the_author_is_not_a_member_of_the_team` |
| author.test.js > should fail when the author is not one_of the members of the team | `tests/upstream_validators_a_l/author.rs::should_fail_when_the_author_is_not_one_of_the_members_of_the_team` |
| author.test.js > should pass when the author is not member of the none_of team | `tests/upstream_validators_a_l/author.rs::should_pass_when_the_author_is_not_member_of_the_none_of_team` |
| author.test.js > should fail when the author is member of the none_of team | `tests/upstream_validators_a_l/author.rs::should_fail_when_the_author_is_member_of_the_none_of_team` |
| author.test.js > should pass when the author is one_of @author | `tests/upstream_validators_a_l/author.rs::should_pass_when_the_author_is_one_of_at_author` |
| author.test.js > should fail when the author is none_of @author | `tests/upstream_validators_a_l/author.rs::should_fail_when_the_author_is_none_of_at_author` |
| baseRef.test.js > validateCheckSuite called for check_suite events | `tests/upstream_validators_a_l/base_ref.rs::validate_check_suite_called_for_check_suite_events` (JS mocks validateCheckSuite; Rust checks the check_suite pull requests are validated and no API call is made) |
| baseRef.test.js > validateStatus called for status events | `tests/upstream_validators_a_l/base_ref.rs::validate_status_called_for_status_events` (JS mocks validateStatus; Rust checks the commit pulls API is used) |
| baseRef.test.js > processOptions called for non-check-suite non-status events | `tests/upstream_validators_a_l/base_ref.rs::process_options_called_for_non_check_suite_non_status_events` (JS mocks processOptions; Rust checks the payload base ref is validated without API calls) |
| baseRef.test.js > fail gracefully if invalid regex | `tests/upstream_validators_a_l/base_ref.rs::fail_gracefully_if_invalid_regex` |
| baseRef.test.js > checks that it fail when exclude regex is in baseRef | `tests/upstream_validators_a_l/base_ref.rs::checks_that_it_fail_when_exclude_regex_is_in_base_ref` |
| baseRef.test.js > checks that it passes when exclude regex is not in baseRef | `tests/upstream_validators_a_l/base_ref.rs::checks_that_it_passes_when_exclude_regex_is_not_in_base_ref` |
| baseRef.test.js > checks that it passes when include regex is in baseRef | `tests/upstream_validators_a_l/base_ref.rs::checks_that_it_passes_when_include_regex_is_in_base_ref` |
| baseRef.test.js > checks that advance setting of must_include works | `tests/upstream_validators_a_l/base_ref.rs::checks_that_advance_setting_of_must_include_works` |
| baseRef.test.js > fail when exclude regex is in baseRef of single check_suite pull request | `tests/upstream_validators_a_l/base_ref.rs::fail_when_exclude_regex_is_in_base_ref_of_single_check_suite_pull_request` |
| baseRef.test.js > fail when exclude regex is in one baseRef of multiple check_suite pull requests | `tests/upstream_validators_a_l/base_ref.rs::fail_when_exclude_regex_is_in_one_base_ref_of_multiple_check_suite_pull_requests` |
| baseRef.test.js > pass when exclude regex is not in any baseRef of multiple check_suite pull requests | `tests/upstream_validators_a_l/base_ref.rs::pass_when_exclude_regex_is_not_in_any_base_ref_of_multiple_check_suite_pull_requests` |
| baseRef.test.js > fail when include regex exists and there are no pull requests in check_suite | `tests/upstream_validators_a_l/base_ref.rs::fail_when_include_regex_exists_and_there_are_no_pull_requests_in_check_suite` |
| baseRef.test.js > fail when exclude regex is in baseRef of single pull request related to status | `tests/upstream_validators_a_l/base_ref.rs::fail_when_exclude_regex_is_in_base_ref_of_single_pull_request_related_to_status` |
| baseRef.test.js > fail when exclude regex is in one baseRef of multiple pull requests related to status | `tests/upstream_validators_a_l/base_ref.rs::fail_when_exclude_regex_is_in_one_base_ref_of_multiple_pull_requests_related_to_status` |
| baseRef.test.js > pass when exclude regex is not in any baseRef of multiple pull requests related to status | `tests/upstream_validators_a_l/base_ref.rs::pass_when_exclude_regex_is_not_in_any_base_ref_of_multiple_pull_requests_related_to_status` |
| baseRef.test.js > fail when include regex exists and there are no pull requests related to status | `tests/upstream_validators_a_l/base_ref.rs::fail_when_include_regex_exists_and_there_are_no_pull_requests_related_to_status` |
| baseRef.test.js > pass when exclude regex is only in baseRef of a closed pull request related to status | `tests/upstream_validators_a_l/base_ref.rs::pass_when_exclude_regex_is_only_in_base_ref_of_a_closed_pull_request_related_to_status` |
| baseRef.test.js > fail with mediaType when exclude regex is in baseRef of single pull request related to status | `tests/upstream_validators_a_l/base_ref.rs::fail_with_media_type_when_exclude_regex_is_in_base_ref_of_single_pull_request_related_to_status` (the mock does not record headers; asserts mediaType is not sent as query (it becomes the Accept header)) |
| changeset.test.js > validate returns correctly | `tests/upstream_validators_a_l/changeset.rs::validate_returns_correctly` |
| changeset.test.js > supports arrays | `tests/upstream_validators_a_l/changeset.rs::supports_arrays` |
| changeset.test.js > fail gracefully if invalid regex | `tests/upstream_validators_a_l/changeset.rs::fail_gracefully_if_invalid_regex` |
| changeset.test.js > mergeable is true if must_include is one of the label | `tests/upstream_validators_a_l/changeset.rs::mergeable_is_true_if_must_include_is_one_of_the_label` |
| changeset.test.js > mergeable is false if must_exclude is one of the label | `tests/upstream_validators_a_l/changeset.rs::mergeable_is_false_if_must_exclude_is_one_of_the_label` |
| changeset.test.js > that it validates ends_with correctly | `tests/upstream_validators_a_l/changeset.rs::that_it_validates_ends_with_correctly` |
| changeset.test.js > that it validates must_include with all:true correctly | `tests/upstream_validators_a_l/changeset.rs::that_it_validates_must_include_with_all_true_correctly` |
| changeset.test.js > that it validates must_exclude with all:true correctly | `tests/upstream_validators_a_l/changeset.rs::that_it_validates_must_exclude_with_all_true_correctly` |
| changeset.test.js > correct files are considered based on file status setting | `tests/upstream_validators_a_l/changeset.rs::correct_files_are_considered_based_on_file_status_setting` |
| commit.test.js > validate returns correctly | `tests/upstream_validators_a_l/commit.rs::validate_returns_correctly` |
| commit.test.js > oldest_only sub option | `tests/upstream_validators_a_l/commit.rs::oldest_only_sub_option` |
| commit.test.js > newest_only sub option | `tests/upstream_validators_a_l/commit.rs::newest_only_sub_option` |
| commit.test.js > skip_merge sub option | `tests/upstream_validators_a_l/commit.rs::skip_merge_sub_option` |
| commit.test.js > single_commit_only sub option | `tests/upstream_validators_a_l/commit.rs::single_commit_only_sub_option` |
| commit.test.js > author_email option should be used | `tests/upstream_validators_a_l/commit.rs::author_email_option_should_be_used` |
| commit.test.js > author_email option should match the regex | `tests/upstream_validators_a_l/commit.rs::author_email_option_should_match_the_regex` |
| commit.test.js > committer_email option should be used | `tests/upstream_validators_a_l/commit.rs::committer_email_option_should_be_used` |
| commit.test.js > committer_email option should match the regex | `tests/upstream_validators_a_l/commit.rs::committer_email_option_should_match_the_regex` |
| contents.test.js > files pr_diff option works correctly | `tests/upstream_validators_a_l/contents.rs::files_pr_diff_option_works_correctly` |
| contents.test.js > files ignore option works correctly | `tests/upstream_validators_a_l/contents.rs::files_ignore_option_works_correctly` |
| contents.test.js > fail gracefully if content is not found | `tests/upstream_validators_a_l/contents.rs::fail_gracefully_if_content_is_not_found` |
| dependent.test.js > dependent files with modified > changed file exists and files found | `tests/upstream_validators_a_l/dependent.rs::dependent_files_with_modified_changed_file_exists_and_files_found` |
| dependent.test.js > dependent files with modified > changed file exist and file(s) not found | `tests/upstream_validators_a_l/dependent.rs::dependent_files_with_modified_changed_file_exists_and_files_not_found` |
| dependent.test.js > dependent files with modified > modified does not exist and file(s) not found | `tests/upstream_validators_a_l/dependent.rs::dependent_files_with_modified_modified_does_not_exist_and_files_not_found` |
| dependent.test.js > dependent files with modified > required sub option works as alias for files | `tests/upstream_validators_a_l/dependent.rs::dependent_files_with_modified_required_sub_option_works_as_alias_for_files` |
| dependent.test.js > dependent files with modified > error when "file" sub option is missing | `tests/upstream_validators_a_l/dependent.rs::dependent_files_with_modified_error_when_file_sub_option_is_missing` |
| dependent.test.js > dependent files with modified > glob works with changed file option | `tests/upstream_validators_a_l/dependent.rs::dependent_files_with_modified_glob_works_with_changed_file_option` |
| dependent.test.js > that mergeable is true if none of the dependent file is modified | `tests/upstream_validators_a_l/dependent.rs::that_mergeable_is_true_if_none_of_the_dependent_file_is_modified` |
| dependent.test.js > that mergeable is true if all of the dependent file is modified | `tests/upstream_validators_a_l/dependent.rs::that_mergeable_is_true_if_all_of_the_dependent_file_is_modified` |
| dependent.test.js > that mergeable is true if one of the dependent file is added | `tests/upstream_validators_a_l/dependent.rs::that_mergeable_is_true_if_one_of_the_dependent_file_is_added` |
| dependent.test.js > that mergeable is false when only one of the dependent file is added | `tests/upstream_validators_a_l/dependent.rs::that_mergeable_is_false_when_only_one_of_the_dependent_file_is_added` |
| dependent.test.js > that mergeable is false when only some of the dependent files are modified | `tests/upstream_validators_a_l/dependent.rs::that_mergeable_is_false_when_only_some_of_the_dependent_files_are_modified` |
| dependent.test.js > test description is correct | `tests/upstream_validators_a_l/dependent.rs::test_description_is_correct` |
| dependent.test.js > test that custom message is correct | `tests/upstream_validators_a_l/dependent.rs::test_that_custom_message_is_correct` |
| description.test.js > isMergeable is true if the PR body is not empty | `tests/upstream_validators_a_l/description.rs::is_mergeable_is_true_if_the_pr_body_is_not_empty` |
| description.test.js > isMergeable is false if the PR body is empty | `tests/upstream_validators_a_l/description.rs::is_mergeable_is_false_if_the_pr_body_is_empty` |
| description.test.js > description is correct | `tests/upstream_validators_a_l/description.rs::description_is_correct` |
| description.test.js > must_include works | `tests/upstream_validators_a_l/description.rs::must_include_works` |
| description.test.js > must_exclude works | `tests/upstream_validators_a_l/description.rs::must_exclude_works` |
| headRef.test.js > fail gracefully if invalid regex | `tests/upstream_validators_a_l/head_ref.rs::fail_gracefully_if_invalid_regex` |
| headRef.test.js > checks that it fail when exclude regex is in headRef | `tests/upstream_validators_a_l/head_ref.rs::checks_that_it_fail_when_exclude_regex_is_in_head_ref` |
| headRef.test.js > checks that advance setting of must_include works | `tests/upstream_validators_a_l/head_ref.rs::checks_that_advance_setting_of_must_include_works` |
| label.test.js > validate returns correctly | `tests/upstream_validators_a_l/label.rs::validate_returns_correctly` |
| label.test.js > fail gracefully if invalid regex | `tests/upstream_validators_a_l/label.rs::fail_gracefully_if_invalid_regex` |
| label.test.js > mergeable is false if regex found or true if not when there is only one label | `tests/upstream_validators_a_l/label.rs::mergeable_is_false_if_regex_found_or_true_if_not_when_there_is_only_one_label` |
| label.test.js > mergeable is false if regex found or true if not when there are multiple labels | `tests/upstream_validators_a_l/label.rs::mergeable_is_false_if_regex_found_or_true_if_not_when_there_are_multiple_labels` |
| label.test.js > description is correct | `tests/upstream_validators_a_l/label.rs::description_is_correct` |
| label.test.js > mergeable is true if must_include is one of the label | `tests/upstream_validators_a_l/label.rs::mergeable_is_true_if_must_include_is_one_of_the_label` |
| label.test.js > mergeable is false if must_exclude is one of the label | `tests/upstream_validators_a_l/label.rs::mergeable_is_false_if_must_exclude_is_one_of_the_label` |
| label.test.js > that it validates ends_with correctly | `tests/upstream_validators_a_l/label.rs::that_it_validates_ends_with_correctly` |
| label.test.js > complex Logic test | `tests/upstream_validators_a_l/label.rs::complex_logic_test` |
| lastComment.test.js > validate returns correctly | `tests/upstream_validators_a_l/last_comment.rs::validate_returns_correctly` |
| lastComment.test.js > validate applies user exclusions correctly | `tests/upstream_validators_a_l/last_comment.rs::validate_applies_user_exclusions_correctly` |
| lastComment.test.js > validate applies bot exclusions correctly | `tests/upstream_validators_a_l/last_comment.rs::validate_applies_bot_exclusions_correctly` |
| lastComment.test.js > validate applies bot exclusions correctly | `tests/upstream_validators_a_l/last_comment.rs::validate_applies_bot_exclusions_correctly_when_no_bots_is_false` (second JS test with the duplicate title "validate applies bot exclusions correctly") |
| lastComment.test.js > validate applies @author exclusions correctly | `tests/upstream_validators_a_l/last_comment.rs::validate_applies_at_author_exclusions_correctly` |
| lastComment.test.js > validate applies @author inclusions correctly | `tests/upstream_validators_a_l/last_comment.rs::validate_applies_at_author_inclusions_correctly` (JS expectation of the 2nd call relies on validate() deleting comment_author from the shared settings object; Rust asserts both the fresh-settings result and the JS sequence) |
| lastComment.test.js > fail gracefully if invalid regex | `tests/upstream_validators_a_l/last_comment.rs::fail_gracefully_if_invalid_regex` |
| lastComment.test.js > description is correct | `tests/upstream_validators_a_l/last_comment.rs::description_is_correct` |
| lastComment.test.js > mergeable is true if must_include is the last comment | `tests/upstream_validators_a_l/last_comment.rs::mergeable_is_true_if_must_include_is_the_last_comment` |
| lastComment.test.js > mergeable is false if must_exclude is one of the lastComment | `tests/upstream_validators_a_l/last_comment.rs::mergeable_is_false_if_must_exclude_is_one_of_the_last_comment` |
| lastComment.test.js > mergeable is true if single comment got edited, disregarding whether its the last comment | `tests/upstream_validators_a_l/last_comment.rs::mergeable_is_true_if_single_comment_got_edited_disregarding_whether_its_the_last_comment` |
| lastComment.test.js > complex Logic test | `tests/upstream_validators_a_l/last_comment.rs::complex_logic_test` (same settings-mutation artifact for the last call; both variants asserted) |
| options_processor/owners.test.js > that * works | `tests/upstream_validators_a_l/owners.rs::that_star_works` |
| options_processor/owners.test.js > that *.js (fileType) works | `tests/upstream_validators_a_l/owners.rs::that_star_dot_js_file_type_works` |
| options_processor/owners.test.js > that /docs/ only matches docs directory in root | `tests/upstream_validators_a_l/owners.rs::that_slash_docs_slash_only_matches_docs_directory_in_root` |
| options_processor/owners.test.js > that /docs/ only matches all files in all subdirectory | `tests/upstream_validators_a_l/owners.rs::that_slash_docs_slash_only_matches_all_files_in_all_subdirectory` |
| options_processor/owners.test.js > that /docs/* only matches all files in that directory | `tests/upstream_validators_a_l/owners.rs::that_slash_docs_slash_star_only_matches_all_files_in_that_directory` |
| options_processor/owners.test.js > that **/docs matches all files and directories in docs/ | `tests/upstream_validators_a_l/owners.rs::that_double_star_slash_docs_matches_all_files_and_directories_in_docs` |
| options_processor/owners.test.js > that /docs/**/apps/ matches the sub directories properly | `tests/upstream_validators_a_l/owners.rs::that_slash_docs_double_star_apps_matches_the_sub_directories_properly` |
| options_processor/owners.test.js > that /apps/** matches all the sub directories of app | `tests/upstream_validators_a_l/owners.rs::that_slash_apps_double_star_matches_all_the_sub_directories_of_app` |
| options_processor/owners.test.js > that apps/ matches all instances of app/ | `tests/upstream_validators_a_l/owners.rs::that_apps_slash_matches_all_instances_of_app` |
| options_processor/owners.test.js > that rules that comes later takes higher priority | `tests/upstream_validators_a_l/owners.rs::that_rules_that_comes_later_takes_higher_priority` |
| options_processor/owners.test.js > teams owners are processed correctly | `tests/upstream_validators_a_l/owners.rs::teams_owners_are_processed_correctly` (JS mocks Teams.extractTeamMemberships; Rust stubs the team membership API (members also listed as individual owners, since memberships are only checked for individuals)) |
| options_processor/owners.test.js > non-team member cannot approve the PR | `tests/upstream_validators_a_l/owners.rs::non_team_member_cannot_approve_the_pr` (membership API stubbed instead of mocking Teams) |
| options_processor/owners.test.js > teams owners and individuals are processed correctly | `tests/upstream_validators_a_l/owners.rs::teams_owners_and_individuals_are_processed_correctly` (membership API stubbed instead of mocking Teams) |
| options_processor/teams.test.js > teams members are extracted properly | `tests/upstream_validators_a_l/teams.rs::teams_members_are_extracted_properly` |
| options_processor/teams.test.js > throws teamNotFound error if at least one team is not found | `tests/upstream_validators_a_l/teams.rs::throws_team_not_found_error_if_at_least_one_team_is_not_found` |
| options_processor/options/and.test.js > return pass if input begins with the rule | `src/validators/options/and.rs::tests::return_pass_if_input_begins_with_the_rule` |
| options_processor/options/and.test.js > return fail if input does not begins with the rule | `src/validators/options/and.rs::tests::return_fail_if_input_does_not_begins_with_the_rule` |
| options_processor/options/and.test.js > return error if inputs are not in expected format | `src/validators/options/and.rs::tests::return_error_if_inputs_are_not_in_expected_format` |
| options_processor/options/and.test.js > return error if option is not valid | `src/validators/options/and.rs::tests::return_error_if_option_is_not_valid` |
| options_processor/options/and.test.js > return error if sub option is not valid | `src/validators/options/and.rs::tests::return_error_if_sub_option_is_not_valid` |
| options_processor/options/and.test.js > return error if multiple sub option is not valid | `src/validators/options/and.rs::tests::return_error_if_multiple_sub_option_is_not_valid` |
| options_processor/options/or.test.js > return pass if input passes one of the OR conditions | `src/validators/options/or.rs::tests::return_pass_if_input_passes_one_of_the_or_conditions` |
| options_processor/options/or.test.js > return fail if none of the input passes one of the OR conditions | `src/validators/options/or.rs::tests::return_fail_if_none_of_the_input_passes_one_of_the_or_conditions` |
| options_processor/options/or.test.js > return error if inputs are not in expected format | `src/validators/options/or.rs::tests::return_error_if_inputs_are_not_in_expected_format` |
| options_processor/options/or.test.js > return error if option is not valid | `src/validators/options/or.rs::tests::return_error_if_option_is_not_valid` |
| options_processor/options/or.test.js > return error if sub option is not valid | `src/validators/options/or.rs::tests::return_error_if_sub_option_is_not_valid` |
| options_processor/options/begins_with.test.js > return pass if input begins with the rule and matches is an array | `src/validators/options/begins_with.rs::tests::return_pass_if_input_begins_with_the_rule_and_matches_is_an_array` |
| options_processor/options/begins_with.test.js > return pass if input begins with the rule | `src/validators/options/begins_with.rs::tests::return_pass_if_input_begins_with_the_rule` |
| options_processor/options/begins_with.test.js > return fail if input does not begins with the rule | `src/validators/options/begins_with.rs::tests::return_fail_if_input_does_not_begins_with_the_rule` |
| options_processor/options/begins_with.test.js > return error if inputs are not in expected format | `src/validators/options/begins_with.rs::tests::return_error_if_inputs_are_not_in_expected_format` |
| options_processor/options/ends_with.test.js > return pass if input ends with the rule and matches is an array | `src/validators/options/ends_with.rs::tests::return_pass_if_input_ends_with_the_rule_and_matches_is_an_array` |
| options_processor/options/ends_with.test.js > return pass if input meets the criteria | `src/validators/options/ends_with.rs::tests::return_pass_if_input_meets_the_criteria` |
| options_processor/options/ends_with.test.js > return fail if input does not meet the criteria | `src/validators/options/ends_with.rs::tests::return_fail_if_input_does_not_meet_the_criteria` |
| options_processor/options/ends_with.test.js > return error if inputs are not in expected format | `src/validators/options/ends_with.rs::tests::return_error_if_inputs_are_not_in_expected_format` |
| options_processor/options/max.test.js > max option applied to arrays > return pass if input meets the criteria | `src/validators/options/max.rs::tests::max_option_applied_to_arrays_return_pass_if_input_meets_the_criteria` |
| options_processor/options/max.test.js > max option applied to arrays > return pass if input exactly meets the criteria | `src/validators/options/max.rs::tests::max_option_applied_to_arrays_return_pass_if_input_exactly_meets_the_criteria` |
| options_processor/options/max.test.js > max option applied to arrays > return fail if input does not meet the criteria | `src/validators/options/max.rs::tests::max_option_applied_to_arrays_return_fail_if_input_does_not_meet_the_criteria` |
| options_processor/options/max.test.js > max option applied to integers > return pass if input meets the criteria | `src/validators/options/max.rs::tests::max_option_applied_to_integers_return_pass_if_input_meets_the_criteria` |
| options_processor/options/max.test.js > max option applied to integers > return pass if input exactly meets criteria | `src/validators/options/max.rs::tests::max_option_applied_to_integers_return_pass_if_input_exactly_meets_criteria` |
| options_processor/options/max.test.js > max option applied to integers > return fail if input does not meet the criteria | `src/validators/options/max.rs::tests::max_option_applied_to_integers_return_fail_if_input_does_not_meet_the_criteria` |
| options_processor/options/max.test.js > max option configured incorrectly > return error if rule config is not in expected format | `src/validators/options/max.rs::tests::max_option_configured_incorrectly_return_error_if_rule_config_is_not_in_expected_format` |
| options_processor/options/max.test.js > max option configured incorrectly > return error if input is not supported type | `src/validators/options/max.rs::tests::max_option_configured_incorrectly_return_error_if_input_is_not_supported_type` |
| options_processor/options/min.test.js > return pass if input meets the criteria | `src/validators/options/min.rs::tests::return_pass_if_input_meets_the_criteria` |
| options_processor/options/min.test.js > return fail if input does not meet the criteria | `src/validators/options/min.rs::tests::return_fail_if_input_does_not_meet_the_criteria` |
| options_processor/options/min.test.js > return error if inputs are not in expected format | `src/validators/options/min.rs::tests::return_error_if_inputs_are_not_in_expected_format` |
| options_processor/options/must_exclude.test.js > return pass if input meets the criteria | `src/validators/options/must_exclude.rs::tests::return_pass_if_input_meets_the_criteria` |
| options_processor/options/must_exclude.test.js > return fail if input does not meet the criteria | `src/validators/options/must_exclude.rs::tests::return_fail_if_input_does_not_meet_the_criteria` |
| options_processor/options/must_exclude.test.js > return error if inputs are not in expected format | `src/validators/options/must_exclude.rs::tests::return_error_if_inputs_are_not_in_expected_format` |
| options_processor/options/must_exclude.test.js > check "all" sub option works | `src/validators/options/must_exclude.rs::tests::check_all_sub_option_works` |
| options_processor/options/must_exclude.test.js > that regex_flag works as expected | `src/validators/options/must_exclude.rs::tests::that_regex_flag_works_as_expected` |
| options_processor/options/must_exclude.test.js > return pass if input array meets the criteria | `src/validators/options/must_exclude.rs::tests::return_pass_if_input_array_meets_the_criteria` |
| options_processor/options/must_exclude.test.js > return fail if input array does not meet the criteria | `src/validators/options/must_exclude.rs::tests::return_fail_if_input_array_does_not_meet_the_criteria` |
| options_processor/options/must_include.test.js > return pass if input meets the criteria | `src/validators/options/must_include.rs::tests::return_pass_if_input_meets_the_criteria` |
| options_processor/options/must_include.test.js > return fail if input does not meet the criteria | `src/validators/options/must_include.rs::tests::return_fail_if_input_does_not_meet_the_criteria` |
| options_processor/options/must_include.test.js > return error if inputs are not in expected format | `src/validators/options/must_include.rs::tests::return_error_if_inputs_are_not_in_expected_format` |
| options_processor/options/must_include.test.js > check "all" sub option works | `src/validators/options/must_include.rs::tests::check_all_sub_option_works` |
| options_processor/options/must_include.test.js > that regex_flag works as expected | `src/validators/options/must_include.rs::tests::that_regex_flag_works_as_expected` |
| options_processor/options/must_include.test.js > return pass if input array meets the criteria | `src/validators/options/must_include.rs::tests::return_pass_if_input_array_meets_the_criteria` |
| options_processor/options/must_include.test.js > return fail if input array does not meet the criteria | `src/validators/options/must_include.rs::tests::return_fail_if_input_array_does_not_meet_the_criteria` |
| options_processor/options/no_empty.test.js > return pass if input meets the criteria | `src/validators/options/no_empty.rs::tests::return_pass_if_input_meets_the_criteria` |
| options_processor/options/no_empty.test.js > return fail if input does not meet the criteria | `src/validators/options/no_empty.rs::tests::return_fail_if_input_does_not_meet_the_criteria` |
| options_processor/options/no_empty.test.js > return error if input does not meet the criteria | `src/validators/options/no_empty.rs::tests::return_error_if_input_does_not_meet_the_criteria` |
| options_processor/options/no_empty.test.js > return error if inputs are not in expected format | `src/validators/options/no_empty.rs::tests::return_error_if_inputs_are_not_in_expected_format` |
| options_processor/options/required.test.js > return pass if input meets the criteria | `src/validators/options/required.rs::tests::return_pass_if_input_meets_the_criteria` |
| options_processor/options/required.test.js > return fail if input does not meet the criteria | `src/validators/options/required.rs::tests::return_fail_if_input_does_not_meet_the_criteria` |
| options_processor/options/required.test.js > return pass when reviewers list not provided | `src/validators/options/required.rs::tests::return_pass_when_reviewers_list_not_provided` |
| options_processor/options/required.test.js > case sensitivity | `src/validators/options/required.rs::tests::case_sensitivity` |
| options_processor/options/required.test.js > case sensitivity with correct case insensitivity. | `src/validators/options/required.rs::tests::case_sensitivity_with_correct_case_insensitivity` |
| options_processor/options/jira.test.js > Return false with wrong ticket with findIssue | `src/validators/options/jira.rs::tests::return_false_with_wrong_ticket_with_find_issue` |
| options_processor/options/jira.test.js > return error if inputs are not in expected format | `src/validators/options/jira.rs::tests::return_error_if_inputs_are_not_in_expected_format` |
| options_processor/options/jira.test.js > return pass if input meets the criteria | not applicable: needs checkTicketStatus to return true, i.e. a reachable Jira answering for the ticket; JIRA_HOST has no port setting (same URL building as the original jira-client), so a local mock Jira cannot be addressed and JS only reached this by mocking checkTicketStatus |
| options_processor/options/jira.test.js > return fail if input doesn't meet the regex criteria | `src/validators/options/jira.rs::tests::return_fail_if_input_does_not_meet_the_regex_criteria` |
| options_processor/options/jira.test.js > return fail if JIRA Ticket ID couldn't be found | `src/validators/options/jira.rs::tests::return_fail_if_jira_ticket_id_could_not_be_found` |
| options_processor/options/jira.test.js > return fail if JIRA Ticket ID couldn't be found | `src/validators/options/jira.rs::tests::return_fail_if_jira_ticket_id_could_not_be_found_without_a_configured_jira_host` (second JS test with the duplicate title; JS mocks checkTicketStatus=false, Rust uses an unconfigured Jira host (the first uses an unreachable one)) |
| options_processor/options/lib/consolidateResults.test.js > returns pass status if no fail or error status exists | `src/validators/options/output.rs::tests::returns_pass_status_if_no_fail_or_error_status_exists` |
| options_processor/options/lib/consolidateResults.test.js > returns fail status if fail status exists but no error status exists | `src/validators/options/output.rs::tests::returns_fail_status_if_fail_status_exists_but_no_error_status_exists` |
| options_processor/options/lib/consolidateResults.test.js > returns error status if error status exists | `src/validators/options/output.rs::tests::returns_error_status_if_error_status_exists` |
| options_processor/options.test.js > return correct output if all inputs are valid | `src/validators/options/mod.rs::tests::valid_rules_pass_for_single_rule_and_rule_list` |
| options_processor/options.test.js > return error if unsupported options are provided | `src/validators/options/mod.rs::tests::unknown_option_reports_missing_module` (existing test; message is the Node runtime text without the Jest-only " from 'lib/...'" suffix) |
| options_processor/options.test.js > return raw output if returnRawOutput is set to true | `src/validators/options/mod.rs::tests::raw_output_is_the_list_of_option_outputs` |
| options_processor/assignees.test.js > that assignees are correctly retrieved | `src/validators/options_processor/assignees.rs::tests::assignee_logins_are_retrieved` |
