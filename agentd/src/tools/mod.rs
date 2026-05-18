pub mod channels;
pub mod common;
pub mod data;
pub mod datadog_api;
pub mod dev_tools;
pub mod discover_tools;
pub mod docker;
pub mod filesystem;
pub mod git;
pub mod github_api;
pub mod http;
pub mod jira_api;
pub mod kubernetes;
pub mod linear_api;
pub mod notion_api;
pub mod package_managers;
pub mod pagerduty_api;
pub mod plugins;
pub mod search;
pub mod sentry_api;
pub mod shell;
pub mod slack_api;
pub mod storage;
pub mod stripe_api;
pub mod utils;
pub mod vercel_api;
pub mod web;

// Re-export core items from common
pub use common::{execute_http_command, resolve_path, Tool, ToolContext, ToolDefinition};
pub use common::{CHANNELS, MEMORY_STORE, SECRET_STORE};

// Re-export all tool types
pub use channels::*;
pub use data::*;
pub use datadog_api::*;
pub use dev_tools::*;
pub use discover_tools::*;
pub use docker::*;
pub use filesystem::*;
pub use git::*;
pub use github_api::*;
pub use http::*;
pub use jira_api::*;
pub use kubernetes::*;
pub use linear_api::*;
pub use notion_api::*;
pub use package_managers::*;
pub use pagerduty_api::*;
pub use search::*;
pub use sentry_api::*;
pub use shell::*;
pub use slack_api::*;
pub use storage::*;
pub use stripe_api::*;
pub use utils::*;
pub use vercel_api::*;
pub use web::*;

// ============== FACTORY FUNCTIONS ==============

pub fn create_read_file_tool() -> Box<dyn Tool> {
    Box::new(filesystem::ReadFileTool)
}
pub fn create_write_file_tool() -> Box<dyn Tool> {
    Box::new(filesystem::WriteFileTool)
}
pub fn create_append_file_tool() -> Box<dyn Tool> {
    Box::new(filesystem::AppendFileTool)
}
pub fn create_delete_file_tool() -> Box<dyn Tool> {
    Box::new(filesystem::DeleteFileTool)
}
pub fn create_copy_file_tool() -> Box<dyn Tool> {
    Box::new(filesystem::CopyFileTool)
}
pub fn create_move_file_tool() -> Box<dyn Tool> {
    Box::new(filesystem::MoveFileTool)
}
pub fn create_list_files_tool() -> Box<dyn Tool> {
    Box::new(filesystem::ListFilesTool)
}
pub fn create_create_directory_tool() -> Box<dyn Tool> {
    Box::new(filesystem::CreateDirectoryTool)
}
pub fn create_delete_directory_tool() -> Box<dyn Tool> {
    Box::new(filesystem::DeleteDirectoryTool)
}
pub fn create_get_file_info_tool() -> Box<dyn Tool> {
    Box::new(filesystem::GetFileInfoTool)
}
pub fn create_file_exists_tool() -> Box<dyn Tool> {
    Box::new(filesystem::FileExistsTool)
}

pub fn create_run_command_tool() -> Box<dyn Tool> {
    Box::new(shell::RunCommandTool)
}
pub fn create_run_script_tool() -> Box<dyn Tool> {
    Box::new(shell::RunScriptTool)
}
pub fn create_kill_process_tool() -> Box<dyn Tool> {
    Box::new(shell::KillProcessTool)
}
pub fn create_get_env_tool() -> Box<dyn Tool> {
    Box::new(shell::GetEnvTool)
}
pub fn create_set_env_tool() -> Box<dyn Tool> {
    Box::new(shell::SetEnvTool)
}

pub fn create_http_get_tool() -> Box<dyn Tool> {
    Box::new(http::HttpGetTool)
}
pub fn create_http_post_tool() -> Box<dyn Tool> {
    Box::new(http::HttpPostTool)
}
pub fn create_http_put_tool() -> Box<dyn Tool> {
    Box::new(http::HttpPutTool)
}
pub fn create_http_delete_tool() -> Box<dyn Tool> {
    Box::new(http::HttpDeleteTool)
}
pub fn create_http_patch_tool() -> Box<dyn Tool> {
    Box::new(http::HttpPatchTool)
}
pub fn create_download_file_tool() -> Box<dyn Tool> {
    Box::new(http::DownloadFileTool)
}

pub fn create_websocket_send_tool() -> Box<dyn Tool> {
    Box::new(http::WebsocketSendTool)
}

pub fn create_json_parse_tool() -> Box<dyn Tool> {
    Box::new(data::JsonParseTool)
}
pub fn create_json_stringify_tool() -> Box<dyn Tool> {
    Box::new(data::JsonStringifyTool)
}
pub fn create_json_query_tool() -> Box<dyn Tool> {
    Box::new(data::JsonQueryTool)
}
pub fn create_csv_read_tool() -> Box<dyn Tool> {
    Box::new(data::CsvReadTool)
}
pub fn create_csv_write_tool() -> Box<dyn Tool> {
    Box::new(data::CsvWriteTool)
}

pub fn create_git_clone_tool() -> Box<dyn Tool> {
    Box::new(git::GitCloneTool)
}
pub fn create_git_status_tool() -> Box<dyn Tool> {
    Box::new(git::GitStatusTool)
}
pub fn create_git_add_tool() -> Box<dyn Tool> {
    Box::new(git::GitAddTool)
}
pub fn create_git_commit_tool() -> Box<dyn Tool> {
    Box::new(git::GitCommitTool)
}
pub fn create_git_push_tool() -> Box<dyn Tool> {
    Box::new(git::GitPushTool)
}
pub fn create_git_pull_tool() -> Box<dyn Tool> {
    Box::new(git::GitPullTool)
}
pub fn create_git_branch_tool() -> Box<dyn Tool> {
    Box::new(git::GitBranchTool)
}
pub fn create_git_checkout_tool() -> Box<dyn Tool> {
    Box::new(git::GitCheckoutTool)
}
pub fn create_git_diff_tool() -> Box<dyn Tool> {
    Box::new(git::GitDiffTool)
}

pub fn create_docker_build_tool() -> Box<dyn Tool> {
    Box::new(docker::DockerBuildTool)
}
pub fn create_docker_run_tool() -> Box<dyn Tool> {
    Box::new(docker::DockerRunTool)
}
pub fn create_docker_stop_tool() -> Box<dyn Tool> {
    Box::new(docker::DockerStopTool)
}
pub fn create_docker_ps_tool() -> Box<dyn Tool> {
    Box::new(docker::DockerPsTool)
}
pub fn create_docker_logs_tool() -> Box<dyn Tool> {
    Box::new(docker::DockerLogsTool)
}
pub fn create_docker_exec_tool() -> Box<dyn Tool> {
    Box::new(docker::DockerExecTool)
}
pub fn create_docker_pull_tool() -> Box<dyn Tool> {
    Box::new(docker::DockerPullTool)
}

pub fn create_kubectl_apply_tool() -> Box<dyn Tool> {
    Box::new(kubernetes::KubectlApplyTool)
}
pub fn create_kubectl_get_tool() -> Box<dyn Tool> {
    Box::new(kubernetes::KubectlGetTool)
}
pub fn create_kubectl_delete_tool() -> Box<dyn Tool> {
    Box::new(kubernetes::KubectlDeleteTool)
}
pub fn create_kubectl_logs_tool() -> Box<dyn Tool> {
    Box::new(kubernetes::KubectlLogsTool)
}
pub fn create_kubectl_exec_tool() -> Box<dyn Tool> {
    Box::new(kubernetes::KubectlExecTool)
}
pub fn create_kubectl_describe_tool() -> Box<dyn Tool> {
    Box::new(kubernetes::KubectlDescribeTool)
}

pub fn create_memory_set_tool() -> Box<dyn Tool> {
    Box::new(storage::MemorySetTool)
}
pub fn create_memory_get_tool() -> Box<dyn Tool> {
    Box::new(storage::MemoryGetTool)
}
pub fn create_memory_delete_tool() -> Box<dyn Tool> {
    Box::new(storage::MemoryDeleteTool)
}
pub fn create_memory_list_tool() -> Box<dyn Tool> {
    Box::new(storage::MemoryListTool)
}
pub fn create_memory_save_tool() -> Box<dyn Tool> {
    Box::new(storage::MemorySaveTool)
}
pub fn create_memory_load_tool() -> Box<dyn Tool> {
    Box::new(storage::MemoryLoadTool)
}

pub fn create_secret_set_tool() -> Box<dyn Tool> {
    Box::new(storage::SecretSetTool)
}
pub fn create_secret_get_tool() -> Box<dyn Tool> {
    Box::new(storage::SecretGetTool)
}

pub fn create_npm_install_tool() -> Box<dyn Tool> {
    Box::new(package_managers::NpmInstallTool)
}
pub fn create_pip_install_tool() -> Box<dyn Tool> {
    Box::new(package_managers::PipInstallTool)
}
pub fn create_cargo_add_tool() -> Box<dyn Tool> {
    Box::new(package_managers::CargoAddTool)
}

pub fn create_web_search_tool() -> Box<dyn Tool> {
    Box::new(web::WebSearchTool)
}
pub fn create_web_fetch_tool() -> Box<dyn Tool> {
    Box::new(web::WebFetchTool)
}
pub fn create_web_screenshot_tool() -> Box<dyn Tool> {
    Box::new(web::WebScreenshotTool)
}

pub fn create_create_channel_tool() -> Box<dyn Tool> {
    Box::new(channels::CreateChannelTool)
}
pub fn create_send_message_tool() -> Box<dyn Tool> {
    Box::new(channels::SendMessageTool)
}
pub fn create_read_messages_tool() -> Box<dyn Tool> {
    Box::new(channels::ReadMessagesTool)
}
pub fn create_broadcast_tool() -> Box<dyn Tool> {
    Box::new(channels::BroadcastTool)
}
pub fn create_wait_for_tool() -> Box<dyn Tool> {
    Box::new(channels::WaitForTool)
}
pub fn create_spawn_agent_tool() -> Box<dyn Tool> {
    Box::new(utils::SpawnAgentTool)
}

pub fn create_lint_tool() -> Box<dyn Tool> {
    Box::new(dev_tools::LintTool)
}
pub fn create_test_tool() -> Box<dyn Tool> {
    Box::new(dev_tools::TestTool)
}
pub fn create_build_tool() -> Box<dyn Tool> {
    Box::new(dev_tools::BuildTool)
}
pub fn create_type_check_tool() -> Box<dyn Tool> {
    Box::new(dev_tools::TypeCheckTool)
}

pub fn create_format_tool() -> Box<dyn Tool> {
    Box::new(dev_tools::FormatTool)
}

pub fn create_echo_tool() -> Box<dyn Tool> {
    Box::new(utils::EchoTool)
}

pub fn create_grep_tool() -> Box<dyn Tool> {
    Box::new(search::GrepTool)
}
pub fn create_find_files_tool() -> Box<dyn Tool> {
    Box::new(search::FindFilesTool)
}
pub fn create_search_code_tool() -> Box<dyn Tool> {
    Box::new(search::SearchCodeTool)
}
pub fn create_read_multiple_files_tool() -> Box<dyn Tool> {
    Box::new(search::ReadMultipleFilesTool)
}

pub fn create_discover_tools_tool() -> Box<dyn Tool> {
    Box::new(discover_tools::DiscoverToolsTool)
}

// ─── GitHub integration ───────────────────────────────────────────────────────

pub fn create_github_list_repos_tool() -> Box<dyn Tool> { Box::new(github_api::GithubListReposTool) }
pub fn create_github_get_repo_tool() -> Box<dyn Tool> { Box::new(github_api::GithubGetRepoTool) }
pub fn create_github_list_issues_tool() -> Box<dyn Tool> { Box::new(github_api::GithubListIssuesTool) }
pub fn create_github_get_issue_tool() -> Box<dyn Tool> { Box::new(github_api::GithubGetIssueTool) }
pub fn create_github_create_issue_tool() -> Box<dyn Tool> { Box::new(github_api::GithubCreateIssueTool) }
pub fn create_github_update_issue_tool() -> Box<dyn Tool> { Box::new(github_api::GithubUpdateIssueTool) }
pub fn create_github_add_issue_comment_tool() -> Box<dyn Tool> { Box::new(github_api::GithubAddIssueCommentTool) }
pub fn create_github_list_pull_requests_tool() -> Box<dyn Tool> { Box::new(github_api::GithubListPullRequestsTool) }
pub fn create_github_get_pull_request_tool() -> Box<dyn Tool> { Box::new(github_api::GithubGetPullRequestTool) }
pub fn create_github_create_pull_request_tool() -> Box<dyn Tool> { Box::new(github_api::GithubCreatePullRequestTool) }
pub fn create_github_merge_pull_request_tool() -> Box<dyn Tool> { Box::new(github_api::GithubMergePullRequestTool) }
pub fn create_github_search_code_tool() -> Box<dyn Tool> { Box::new(github_api::GithubSearchCodeTool) }
pub fn create_github_search_issues_tool() -> Box<dyn Tool> { Box::new(github_api::GithubSearchIssuesTool) }
pub fn create_github_get_file_contents_tool() -> Box<dyn Tool> { Box::new(github_api::GithubGetFileContentsTool) }
pub fn create_github_list_workflow_runs_tool() -> Box<dyn Tool> { Box::new(github_api::GithubListWorkflowRunsTool) }
pub fn create_github_get_commit_tool() -> Box<dyn Tool> { Box::new(github_api::GithubGetCommitTool) }

// ─── Linear integration ───────────────────────────────────────────────────────

pub fn create_linear_list_teams_tool() -> Box<dyn Tool> { Box::new(linear_api::LinearListTeamsTool) }
pub fn create_linear_list_issues_tool() -> Box<dyn Tool> { Box::new(linear_api::LinearListIssuesTool) }
pub fn create_linear_get_issue_tool() -> Box<dyn Tool> { Box::new(linear_api::LinearGetIssueTool) }
pub fn create_linear_create_issue_tool() -> Box<dyn Tool> { Box::new(linear_api::LinearCreateIssueTool) }
pub fn create_linear_update_issue_tool() -> Box<dyn Tool> { Box::new(linear_api::LinearUpdateIssueTool) }
pub fn create_linear_add_comment_tool() -> Box<dyn Tool> { Box::new(linear_api::LinearAddCommentTool) }
pub fn create_linear_list_projects_tool() -> Box<dyn Tool> { Box::new(linear_api::LinearListProjectsTool) }
pub fn create_linear_list_workflow_states_tool() -> Box<dyn Tool> { Box::new(linear_api::LinearListWorkflowStatesTool) }

// ─── Slack integration ────────────────────────────────────────────────────────

pub fn create_slack_post_message_tool() -> Box<dyn Tool> { Box::new(slack_api::SlackPostMessageTool) }
pub fn create_slack_list_channels_tool() -> Box<dyn Tool> { Box::new(slack_api::SlackListChannelsTool) }
pub fn create_slack_get_channel_history_tool() -> Box<dyn Tool> { Box::new(slack_api::SlackGetChannelHistoryTool) }
pub fn create_slack_search_messages_tool() -> Box<dyn Tool> { Box::new(slack_api::SlackSearchMessagesTool) }
pub fn create_slack_add_reaction_tool() -> Box<dyn Tool> { Box::new(slack_api::SlackAddReactionTool) }
pub fn create_slack_upload_file_tool() -> Box<dyn Tool> { Box::new(slack_api::SlackUploadFileTool) }
pub fn create_slack_get_user_info_tool() -> Box<dyn Tool> { Box::new(slack_api::SlackGetUserInfoTool) }
pub fn create_slack_update_message_tool() -> Box<dyn Tool> { Box::new(slack_api::SlackUpdateMessageTool) }

// ─── Jira integration ─────────────────────────────────────────────────────────

pub fn create_jira_list_projects_tool() -> Box<dyn Tool> { Box::new(jira_api::JiraListProjectsTool) }
pub fn create_jira_search_issues_tool() -> Box<dyn Tool> { Box::new(jira_api::JiraSearchIssuesTool) }
pub fn create_jira_get_issue_tool() -> Box<dyn Tool> { Box::new(jira_api::JiraGetIssueTool) }
pub fn create_jira_create_issue_tool() -> Box<dyn Tool> { Box::new(jira_api::JiraCreateIssueTool) }
pub fn create_jira_update_issue_tool() -> Box<dyn Tool> { Box::new(jira_api::JiraUpdateIssueTool) }
pub fn create_jira_add_comment_tool() -> Box<dyn Tool> { Box::new(jira_api::JiraAddCommentTool) }
pub fn create_jira_transition_issue_tool() -> Box<dyn Tool> { Box::new(jira_api::JiraTransitionIssueTool) }
pub fn create_jira_get_transitions_tool() -> Box<dyn Tool> { Box::new(jira_api::JiraGetTransitionsTool) }

// ─── Notion integration ───────────────────────────────────────────────────────

pub fn create_notion_search_tool() -> Box<dyn Tool> { Box::new(notion_api::NotionSearchTool) }
pub fn create_notion_get_page_tool() -> Box<dyn Tool> { Box::new(notion_api::NotionGetPageTool) }
pub fn create_notion_create_page_tool() -> Box<dyn Tool> { Box::new(notion_api::NotionCreatePageTool) }
pub fn create_notion_append_block_tool() -> Box<dyn Tool> { Box::new(notion_api::NotionAppendBlockTool) }
pub fn create_notion_query_database_tool() -> Box<dyn Tool> { Box::new(notion_api::NotionQueryDatabaseTool) }
pub fn create_notion_update_page_tool() -> Box<dyn Tool> { Box::new(notion_api::NotionUpdatePageTool) }

// ─── Sentry integration ───────────────────────────────────────────────────────

pub fn create_sentry_list_projects_tool() -> Box<dyn Tool> { Box::new(sentry_api::SentryListProjectsTool) }
pub fn create_sentry_list_issues_tool() -> Box<dyn Tool> { Box::new(sentry_api::SentryListIssuesTool) }
pub fn create_sentry_get_issue_tool() -> Box<dyn Tool> { Box::new(sentry_api::SentryGetIssueTool) }
pub fn create_sentry_update_issue_tool() -> Box<dyn Tool> { Box::new(sentry_api::SentryUpdateIssueTool) }
pub fn create_sentry_list_events_tool() -> Box<dyn Tool> { Box::new(sentry_api::SentryListEventsTool) }
pub fn create_sentry_get_event_tool() -> Box<dyn Tool> { Box::new(sentry_api::SentryGetEventTool) }
pub fn create_sentry_list_releases_tool() -> Box<dyn Tool> { Box::new(sentry_api::SentryListReleasesTool) }

// ─── Stripe integration ───────────────────────────────────────────────────────

pub fn create_stripe_list_customers_tool() -> Box<dyn Tool> { Box::new(stripe_api::StripeListCustomersTool) }
pub fn create_stripe_get_customer_tool() -> Box<dyn Tool> { Box::new(stripe_api::StripeGetCustomerTool) }
pub fn create_stripe_list_payment_intents_tool() -> Box<dyn Tool> { Box::new(stripe_api::StripeListPaymentIntentsTool) }
pub fn create_stripe_get_payment_intent_tool() -> Box<dyn Tool> { Box::new(stripe_api::StripeGetPaymentIntentTool) }
pub fn create_stripe_list_subscriptions_tool() -> Box<dyn Tool> { Box::new(stripe_api::StripeListSubscriptionsTool) }
pub fn create_stripe_list_products_tool() -> Box<dyn Tool> { Box::new(stripe_api::StripeListProductsTool) }
pub fn create_stripe_list_invoices_tool() -> Box<dyn Tool> { Box::new(stripe_api::StripeListInvoicesTool) }
pub fn create_stripe_create_customer_tool() -> Box<dyn Tool> { Box::new(stripe_api::StripeCreateCustomerTool) }

// ─── Vercel integration ───────────────────────────────────────────────────────

pub fn create_vercel_list_projects_tool() -> Box<dyn Tool> { Box::new(vercel_api::VercelListProjectsTool) }
pub fn create_vercel_get_project_tool() -> Box<dyn Tool> { Box::new(vercel_api::VercelGetProjectTool) }
pub fn create_vercel_list_deployments_tool() -> Box<dyn Tool> { Box::new(vercel_api::VercelListDeploymentsTool) }
pub fn create_vercel_get_deployment_tool() -> Box<dyn Tool> { Box::new(vercel_api::VercelGetDeploymentTool) }
pub fn create_vercel_get_deployment_logs_tool() -> Box<dyn Tool> { Box::new(vercel_api::VercelGetDeploymentLogsTool) }
pub fn create_vercel_cancel_deployment_tool() -> Box<dyn Tool> { Box::new(vercel_api::VercelCancelDeploymentTool) }
pub fn create_vercel_list_domains_tool() -> Box<dyn Tool> { Box::new(vercel_api::VercelListDomainsTool) }
pub fn create_vercel_list_env_vars_tool() -> Box<dyn Tool> { Box::new(vercel_api::VercelListEnvVarsTool) }
pub fn create_vercel_create_deployment_tool() -> Box<dyn Tool> { Box::new(vercel_api::VercelCreateDeploymentTool) }

// ─── PagerDuty integration ────────────────────────────────────────────────────

pub fn create_pagerduty_list_incidents_tool() -> Box<dyn Tool> { Box::new(pagerduty_api::PagerDutyListIncidentsTool) }
pub fn create_pagerduty_get_incident_tool() -> Box<dyn Tool> { Box::new(pagerduty_api::PagerDutyGetIncidentTool) }
pub fn create_pagerduty_acknowledge_incident_tool() -> Box<dyn Tool> { Box::new(pagerduty_api::PagerDutyAcknowledgeIncidentTool) }
pub fn create_pagerduty_resolve_incident_tool() -> Box<dyn Tool> { Box::new(pagerduty_api::PagerDutyResolveIncidentTool) }
pub fn create_pagerduty_add_note_tool() -> Box<dyn Tool> { Box::new(pagerduty_api::PagerDutyAddNoteTool) }
pub fn create_pagerduty_list_services_tool() -> Box<dyn Tool> { Box::new(pagerduty_api::PagerDutyListServicesTool) }
pub fn create_pagerduty_list_oncalls_tool() -> Box<dyn Tool> { Box::new(pagerduty_api::PagerDutyListOnCallsTool) }

// ─── Datadog integration ──────────────────────────────────────────────────────

pub fn create_datadog_list_monitors_tool() -> Box<dyn Tool> { Box::new(datadog_api::DatadogListMonitorsTool) }
pub fn create_datadog_get_monitor_tool() -> Box<dyn Tool> { Box::new(datadog_api::DatadogGetMonitorTool) }
pub fn create_datadog_query_metrics_tool() -> Box<dyn Tool> { Box::new(datadog_api::DatadogQueryMetricsTool) }
pub fn create_datadog_list_dashboards_tool() -> Box<dyn Tool> { Box::new(datadog_api::DatadogListDashboardsTool) }
pub fn create_datadog_list_logs_tool() -> Box<dyn Tool> { Box::new(datadog_api::DatadogListLogsTool) }
pub fn create_datadog_list_events_tool() -> Box<dyn Tool> { Box::new(datadog_api::DatadogListEventsTool) }
pub fn create_datadog_mute_monitor_tool() -> Box<dyn Tool> { Box::new(datadog_api::DatadogMuteMonitorTool) }
