pub mod channels;
pub mod common;
pub mod data;
pub mod dev_tools;
pub mod docker;
pub mod filesystem;
pub mod git;
pub mod http;
pub mod kubernetes;
pub mod package_managers;
pub mod shell;
pub mod storage;
pub mod utils;
pub mod web;

// Re-export core items from common
pub use common::{execute_http_command, resolve_path, Tool, ToolContext, ToolDefinition};
pub use common::{CHANNELS, MEMORY_STORE, SECRET_STORE};

// Re-export all tool types
pub use channels::*;
pub use data::*;
pub use dev_tools::*;
pub use docker::*;
pub use filesystem::*;
pub use git::*;
pub use http::*;
pub use kubernetes::*;
pub use package_managers::*;
pub use shell::*;
pub use storage::*;
pub use utils::*;
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

/// Create a set of default tools for all sandboxes
pub fn create_default_tools() -> Vec<Box<dyn Tool>> {
    vec![
        create_read_file_tool(),
        create_write_file_tool(),
        create_append_file_tool(),
        create_delete_file_tool(),
        create_copy_file_tool(),
        create_move_file_tool(),
        create_list_files_tool(),
        create_create_directory_tool(),
        create_delete_directory_tool(),
        create_get_file_info_tool(),
        create_file_exists_tool(),
        create_run_command_tool(),
        create_run_script_tool(),
        create_kill_process_tool(),
        create_get_env_tool(),
        create_set_env_tool(),
        create_http_get_tool(),
        create_http_post_tool(),
        create_http_put_tool(),
        create_http_delete_tool(),
        create_http_patch_tool(),
        create_download_file_tool(),
        create_websocket_send_tool(),
        create_json_parse_tool(),
        create_json_stringify_tool(),
        create_json_query_tool(),
        create_csv_read_tool(),
        create_csv_write_tool(),
        create_git_clone_tool(),
        create_git_status_tool(),
        create_git_add_tool(),
        create_git_commit_tool(),
        create_git_push_tool(),
        create_git_pull_tool(),
        create_git_branch_tool(),
        create_git_checkout_tool(),
        create_git_diff_tool(),
        create_docker_build_tool(),
        create_docker_run_tool(),
        create_docker_stop_tool(),
        create_docker_ps_tool(),
        create_docker_logs_tool(),
        create_docker_exec_tool(),
        create_docker_pull_tool(),
        create_kubectl_apply_tool(),
        create_kubectl_get_tool(),
        create_kubectl_delete_tool(),
        create_kubectl_logs_tool(),
        create_kubectl_exec_tool(),
        create_kubectl_describe_tool(),
        create_memory_set_tool(),
        create_memory_get_tool(),
        create_memory_delete_tool(),
        create_memory_list_tool(),
        create_memory_save_tool(),
        create_memory_load_tool(),
        create_secret_set_tool(),
        create_secret_get_tool(),
        create_npm_install_tool(),
        create_pip_install_tool(),
        create_cargo_add_tool(),
        create_web_search_tool(),
        create_web_fetch_tool(),
        create_web_screenshot_tool(),
        create_create_channel_tool(),
        create_send_message_tool(),
        create_read_messages_tool(),
        create_broadcast_tool(),
        create_wait_for_tool(),
        create_spawn_agent_tool(),
        create_lint_tool(),
        create_test_tool(),
        create_build_tool(),
        create_type_check_tool(),
        create_format_tool(),
        create_echo_tool(),
    ]
}
