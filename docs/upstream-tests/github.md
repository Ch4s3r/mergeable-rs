# Upstream tests: github (API wrapper, client, auth, throttling)

Source: `__tests__/unit/github/` of mergeability/mergeable (one file, `api.test.js`).

- Upstream tests: 68
- Ported: 68
- Not applicable: 0

All ported tests are unit tests in `src/github/api.rs` (`mod tests`). They call the Rust API wrapper against a
local stub GitHub API (`src/github/stub_server.rs`, test-only) instead of the Jest octokit mock: the stub answers
with the status and body the Jest mock returned or rejected with (`{ status: 402 }` becomes an HTTP 402 response,
`{ status: 404 }` an HTTP 404 response).

Notes:
- "that pagination is used" asserted that `octokit.paginate` was called; the Rust port checks the observable
  behaviour instead: a response with a `Link: rel="next"` header is followed and both pages are returned.
- `getIssues > that 404 are simply returned null` stubbed `issues.getIssue`, which the original never calls; the
  port tests what the title says (a 404 from `GET /repos/{owner}/{repo}/issues/{issue_number}` yields no issue).
- `listCommits > that error are NOT re-thrown` asserts that the error is re-thrown despite its title; the port
  follows the assertion.
- No behaviour differences were found; no production code was changed.

| Upstream test (file > describe > title) | Rust test (path::name) or 'not applicable: reason' |
|---|---|
| api.test.js > listFiles > return correct data if no error | `mergeable::github::api::tests::list_files_returns_the_changed_files` |
| api.test.js > listFiles > that pagination is used | `mergeable::github::api::tests::list_files_collects_every_page` |
| api.test.js > listFiles > that error are re-thrown | `mergeable::github::api::tests::list_files_rethrows_errors` |
| api.test.js > listFiles > searchIssuesAndPR > return correct data if no error | `mergeable::github::api::tests::search_issues_and_pull_requests_returns_the_found_items` |
| api.test.js > listFiles > searchIssuesAndPR > that error are re-thrown | `mergeable::github::api::tests::search_issues_and_pull_requests_rethrows_errors` |
| api.test.js > listFiles > getContent > return correct data if no error | `mergeable::github::api::tests::fetch_content_returns_the_decoded_file_content` |
| api.test.js > listFiles > getContent > that 404 are simply returned null | `mergeable::github::api::tests::fetch_content_returns_none_for_a_missing_file` |
| api.test.js > listFiles > getContent > that error are re-thrown | `mergeable::github::api::tests::fetch_content_rethrows_errors` |
| api.test.js > createChecks > return correct data if no error | `mergeable::github::api::tests::create_checks_returns_the_created_check_run` |
| api.test.js > createChecks > that error are re-thrown | `mergeable::github::api::tests::create_checks_rethrows_errors` |
| api.test.js > updateChecks > return correct data if no error | `mergeable::github::api::tests::update_checks_returns_the_response` |
| api.test.js > updateChecks > that error are re-thrown | `mergeable::github::api::tests::update_checks_rethrows_errors` |
| api.test.js > listLabelsOnIssue > return correct data if no error | `mergeable::github::api::tests::list_labels_on_issue_returns_the_label_names` |
| api.test.js > listLabelsOnIssue > that error are re-thrown | `mergeable::github::api::tests::list_labels_on_issue_rethrows_errors` |
| api.test.js > addLabels > return correct data if no error | `mergeable::github::api::tests::add_labels_returns_the_response` |
| api.test.js > addLabels > that error are re-thrown | `mergeable::github::api::tests::add_labels_rethrows_errors` |
| api.test.js > setLabels > return correct data if no error | `mergeable::github::api::tests::set_labels_returns_the_response` |
| api.test.js > setLabels > that error are re-thrown | `mergeable::github::api::tests::set_labels_rethrows_errors` |
| api.test.js > addAssignees > return correct data if no error | `mergeable::github::api::tests::add_assignees_returns_the_response` |
| api.test.js > addAssignees > that error are re-thrown | `mergeable::github::api::tests::add_assignees_rethrows_errors` |
| api.test.js > checkUserCanBeAssigned > return correct data if no error | `mergeable::github::api::tests::check_user_can_be_assigned_returns_the_assignable_user` |
| api.test.js > checkUserCanBeAssigned > that 404 are simply returned null | `mergeable::github::api::tests::check_user_can_be_assigned_returns_none_when_not_found` |
| api.test.js > checkUserCanBeAssigned > that error are re-thrown | `mergeable::github::api::tests::check_user_can_be_assigned_rethrows_errors` |
| api.test.js > createComment > return correct data if no error | `mergeable::github::api::tests::create_comment_returns_the_response` |
| api.test.js > createComment > that error are re-thrown | `mergeable::github::api::tests::create_comment_rethrows_errors` |
| api.test.js > listComments > return correct data if no error | `mergeable::github::api::tests::list_comments_returns_the_comments` |
| api.test.js > listComments > that pagination is used | `mergeable::github::api::tests::list_comments_collects_every_page` |
| api.test.js > listComments > that error are re-thrown | `mergeable::github::api::tests::list_comments_rethrows_errors` |
| api.test.js > deleteComment > return correct data if no error | `mergeable::github::api::tests::delete_comment_returns_the_response` |
| api.test.js > deleteComment > that error are NOT thrown for 404 | `mergeable::github::api::tests::delete_comment_does_not_throw_when_the_comment_is_not_found` |
| api.test.js > deleteComment > that error are re-thrown | `mergeable::github::api::tests::delete_comment_rethrows_errors` |
| api.test.js > updateIssues > return correct data if no error | `mergeable::github::api::tests::update_issue_state_returns_the_response` |
| api.test.js > updateIssues > that error are re-thrown | `mergeable::github::api::tests::update_issue_state_rethrows_errors` |
| api.test.js > getIssues > return correct data if no error | `mergeable::github::api::tests::fetch_issue_returns_the_issue` |
| api.test.js > getIssues > that 404 are simply returned null | `mergeable::github::api::tests::fetch_issue_returns_none_when_not_found` |
| api.test.js > getIssues > that error are re-thrown | `mergeable::github::api::tests::fetch_issue_rethrows_errors` |
| api.test.js > listMembersInOrg > return correct data if no error | `mergeable::github::api::tests::list_members_in_org_returns_the_member_logins` |
| api.test.js > listMembersInOrg > that error are re-thrown | `mergeable::github::api::tests::list_members_in_org_rethrows_errors` |
| api.test.js > getMembershipForUserInOrg > return correct data if no error | `mergeable::github::api::tests::fetch_membership_for_user_in_org_is_true_only_for_an_active_membership` |
| api.test.js > getMembershipForUserInOrg > that error are re-thrown | `mergeable::github::api::tests::fetch_membership_for_user_in_org_rethrows_errors` |
| api.test.js > projectListColumns > return correct data if no error | `mergeable::github::api::tests::project_list_columns_returns_the_column_ids` |
| api.test.js > projectListColumns > that error are re-thrown | `mergeable::github::api::tests::project_list_columns_rethrows_errors` |
| api.test.js > projectListForRepo > return correct data if no error | `mergeable::github::api::tests::project_list_for_repo_returns_the_project_ids_and_names` |
| api.test.js > projectListForRepo > that error are re-thrown | `mergeable::github::api::tests::project_list_for_repo_rethrows_errors` |
| api.test.js > projectListCards > return correct data if no error | `mergeable::github::api::tests::project_list_cards_returns_the_response` |
| api.test.js > projectListCards > that error are re-thrown | `mergeable::github::api::tests::project_list_cards_rethrows_errors` |
| api.test.js > listCollaborators > return correct data if no error | `mergeable::github::api::tests::list_collaborators_returns_the_collaborator_logins` |
| api.test.js > listCollaborators > that error are re-thrown | `mergeable::github::api::tests::list_collaborators_rethrows_errors` |
| api.test.js > getAllTopics > return correct data if no error | `mergeable::github::api::tests::fetch_all_topics_returns_the_topic_names` |
| api.test.js > getAllTopics > that error are re-thrown | `mergeable::github::api::tests::fetch_all_topics_rethrows_errors` |
| api.test.js > compareCommits > return correct data if no error | `mergeable::github::api::tests::compare_commits_returns_the_comparison` |
| api.test.js > compareCommits > that error are re-thrown | `mergeable::github::api::tests::compare_commits_rethrows_errors` |
| api.test.js > checkIfMerged > return correct data if no error | `mergeable::github::api::tests::check_if_merged_is_true_when_merged_and_false_when_not_found` |
| api.test.js > checkIfMerged > that error are re-thrown | `mergeable::github::api::tests::check_if_merged_rethrows_errors` |
| api.test.js > mergePR > return correct data if no error | `mergeable::github::api::tests::merge_pull_request_returns_the_response` |
| api.test.js > mergePR > that error are re-thrown | `mergeable::github::api::tests::merge_pull_request_rethrows_errors` |
| api.test.js > requestReviewers > return correct data if no error | `mergeable::github::api::tests::request_reviewers_returns_the_response` |
| api.test.js > requestReviewers > that error are NOT re-thrown | `mergeable::github::api::tests::request_reviewers_does_not_rethrow_errors` |
| api.test.js > getPR > return correct data if no error | `mergeable::github::api::tests::fetch_pull_request_returns_the_pull_request` |
| api.test.js > getPR > that error are re-thrown | `mergeable::github::api::tests::fetch_pull_request_rethrows_errors` |
| api.test.js > listPR > return correct data if no error | `mergeable::github::api::tests::list_pull_requests_returns_the_response` |
| api.test.js > listPR > that error are re-thrown | `mergeable::github::api::tests::list_pull_requests_rethrows_errors` |
| api.test.js > listReviews > return correct data if no error | `mergeable::github::api::tests::list_reviews_returns_the_reviews` |
| api.test.js > listReviews > that pagination is used | `mergeable::github::api::tests::list_reviews_collects_every_page` |
| api.test.js > listReviews > that error are re-thrown | `mergeable::github::api::tests::list_reviews_rethrows_errors` |
| api.test.js > listCommits > return correct data if no error | `mergeable::github::api::tests::list_commits_returns_message_date_author_and_committer` |
| api.test.js > listCommits > that pagination is used | `mergeable::github::api::tests::list_commits_collects_every_page` |
| api.test.js > listCommits > that error are NOT re-thrown | `mergeable::github::api::tests::list_commits_rethrows_errors` |
