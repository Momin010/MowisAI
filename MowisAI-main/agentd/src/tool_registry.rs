use crate::tools::Tool;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Mutex;

lazy_static! {
    /// Global tool registry mapping tool names to factory functions
    static ref TOOL_REGISTRY: ToolRegistry = ToolRegistry::new();
}

/// Type alias for tool factory functions
type ToolFactory = fn() -> Box<dyn Tool>;

/// Registry for managing all available tools
pub struct ToolRegistry {
    tools: Mutex<HashMap<&'static str, ToolFactory>>,
}

impl ToolRegistry {
    /// Create a new tool registry with all 75 tools registered
    fn new() -> Self {
        let mut tools = HashMap::new();

        // Filesystem tools (11)
        tools.insert(
            "read_file",
            crate::tools::create_read_file_tool as ToolFactory,
        );
        tools.insert(
            "write_file",
            crate::tools::create_write_file_tool as ToolFactory,
        );
        tools.insert(
            "append_file",
            crate::tools::create_append_file_tool as ToolFactory,
        );
        tools.insert(
            "delete_file",
            crate::tools::create_delete_file_tool as ToolFactory,
        );
        tools.insert(
            "copy_file",
            crate::tools::create_copy_file_tool as ToolFactory,
        );
        tools.insert(
            "move_file",
            crate::tools::create_move_file_tool as ToolFactory,
        );
        tools.insert(
            "list_files",
            crate::tools::create_list_files_tool as ToolFactory,
        );
        tools.insert(
            "create_directory",
            crate::tools::create_create_directory_tool as ToolFactory,
        );
        tools.insert(
            "delete_directory",
            crate::tools::create_delete_directory_tool as ToolFactory,
        );
        tools.insert(
            "get_file_info",
            crate::tools::create_get_file_info_tool as ToolFactory,
        );
        tools.insert(
            "file_exists",
            crate::tools::create_file_exists_tool as ToolFactory,
        );

        // Shell tools (5)
        tools.insert(
            "run_command",
            crate::tools::create_run_command_tool as ToolFactory,
        );
        tools.insert(
            "run_script",
            crate::tools::create_run_script_tool as ToolFactory,
        );
        tools.insert(
            "kill_process",
            crate::tools::create_kill_process_tool as ToolFactory,
        );
        tools.insert("get_env", crate::tools::create_get_env_tool as ToolFactory);
        tools.insert("set_env", crate::tools::create_set_env_tool as ToolFactory);

        // HTTP tools (6)
        tools.insert(
            "http_get",
            crate::tools::create_http_get_tool as ToolFactory,
        );
        tools.insert(
            "http_post",
            crate::tools::create_http_post_tool as ToolFactory,
        );
        tools.insert(
            "http_put",
            crate::tools::create_http_put_tool as ToolFactory,
        );
        tools.insert(
            "http_delete",
            crate::tools::create_http_delete_tool as ToolFactory,
        );
        tools.insert(
            "http_patch",
            crate::tools::create_http_patch_tool as ToolFactory,
        );
        tools.insert(
            "download_file",
            crate::tools::create_download_file_tool as ToolFactory,
        );

        // WebSocket tool (1)
        tools.insert(
            "websocket_send",
            crate::tools::create_websocket_send_tool as ToolFactory,
        );

        // Data tools (5)
        tools.insert(
            "json_parse",
            crate::tools::create_json_parse_tool as ToolFactory,
        );
        tools.insert(
            "json_stringify",
            crate::tools::create_json_stringify_tool as ToolFactory,
        );
        tools.insert(
            "json_query",
            crate::tools::create_json_query_tool as ToolFactory,
        );
        tools.insert(
            "csv_read",
            crate::tools::create_csv_read_tool as ToolFactory,
        );
        tools.insert(
            "csv_write",
            crate::tools::create_csv_write_tool as ToolFactory,
        );

        // Git tools (9)
        tools.insert(
            "git_clone",
            crate::tools::create_git_clone_tool as ToolFactory,
        );
        tools.insert(
            "git_status",
            crate::tools::create_git_status_tool as ToolFactory,
        );
        tools.insert("git_add", crate::tools::create_git_add_tool as ToolFactory);
        tools.insert(
            "git_commit",
            crate::tools::create_git_commit_tool as ToolFactory,
        );
        tools.insert(
            "git_push",
            crate::tools::create_git_push_tool as ToolFactory,
        );
        tools.insert(
            "git_pull",
            crate::tools::create_git_pull_tool as ToolFactory,
        );
        tools.insert(
            "git_branch",
            crate::tools::create_git_branch_tool as ToolFactory,
        );
        tools.insert(
            "git_checkout",
            crate::tools::create_git_checkout_tool as ToolFactory,
        );
        tools.insert(
            "git_diff",
            crate::tools::create_git_diff_tool as ToolFactory,
        );

        // Docker tools (7)
        tools.insert(
            "docker_build",
            crate::tools::create_docker_build_tool as ToolFactory,
        );
        tools.insert(
            "docker_run",
            crate::tools::create_docker_run_tool as ToolFactory,
        );
        tools.insert(
            "docker_stop",
            crate::tools::create_docker_stop_tool as ToolFactory,
        );
        tools.insert(
            "docker_ps",
            crate::tools::create_docker_ps_tool as ToolFactory,
        );
        tools.insert(
            "docker_logs",
            crate::tools::create_docker_logs_tool as ToolFactory,
        );
        tools.insert(
            "docker_exec",
            crate::tools::create_docker_exec_tool as ToolFactory,
        );
        tools.insert(
            "docker_pull",
            crate::tools::create_docker_pull_tool as ToolFactory,
        );

        // Kubernetes tools (6)
        tools.insert(
            "kubectl_apply",
            crate::tools::create_kubectl_apply_tool as ToolFactory,
        );
        tools.insert(
            "kubectl_get",
            crate::tools::create_kubectl_get_tool as ToolFactory,
        );
        tools.insert(
            "kubectl_delete",
            crate::tools::create_kubectl_delete_tool as ToolFactory,
        );
        tools.insert(
            "kubectl_logs",
            crate::tools::create_kubectl_logs_tool as ToolFactory,
        );
        tools.insert(
            "kubectl_exec",
            crate::tools::create_kubectl_exec_tool as ToolFactory,
        );
        tools.insert(
            "kubectl_describe",
            crate::tools::create_kubectl_describe_tool as ToolFactory,
        );

        // Memory tools (6)
        tools.insert(
            "memory_set",
            crate::tools::create_memory_set_tool as ToolFactory,
        );
        tools.insert(
            "memory_get",
            crate::tools::create_memory_get_tool as ToolFactory,
        );
        tools.insert(
            "memory_delete",
            crate::tools::create_memory_delete_tool as ToolFactory,
        );
        tools.insert(
            "memory_list",
            crate::tools::create_memory_list_tool as ToolFactory,
        );
        tools.insert(
            "memory_save",
            crate::tools::create_memory_save_tool as ToolFactory,
        );
        tools.insert(
            "memory_load",
            crate::tools::create_memory_load_tool as ToolFactory,
        );

        // Secrets tools (2)
        tools.insert(
            "secret_set",
            crate::tools::create_secret_set_tool as ToolFactory,
        );
        tools.insert(
            "secret_get",
            crate::tools::create_secret_get_tool as ToolFactory,
        );

        // Package tools (3)
        tools.insert(
            "npm_install",
            crate::tools::create_npm_install_tool as ToolFactory,
        );
        tools.insert(
            "pip_install",
            crate::tools::create_pip_install_tool as ToolFactory,
        );
        tools.insert(
            "cargo_add",
            crate::tools::create_cargo_add_tool as ToolFactory,
        );

        // Web tools (3)
        tools.insert(
            "web_search",
            crate::tools::create_web_search_tool as ToolFactory,
        );
        tools.insert(
            "web_fetch",
            crate::tools::create_web_fetch_tool as ToolFactory,
        );
        tools.insert(
            "web_screenshot",
            crate::tools::create_web_screenshot_tool as ToolFactory,
        );

        // Agent coordination tools (6)
        tools.insert(
            "create_channel",
            crate::tools::create_create_channel_tool as ToolFactory,
        );
        tools.insert(
            "send_message",
            crate::tools::create_send_message_tool as ToolFactory,
        );
        tools.insert(
            "read_messages",
            crate::tools::create_read_messages_tool as ToolFactory,
        );
        tools.insert(
            "broadcast",
            crate::tools::create_broadcast_tool as ToolFactory,
        );
        tools.insert(
            "wait_for",
            crate::tools::create_wait_for_tool as ToolFactory,
        );
        tools.insert(
            "spawn_agent",
            crate::tools::create_spawn_agent_tool as ToolFactory,
        );

        // Code analysis tools (6)
        tools.insert("lint", crate::tools::create_lint_tool as ToolFactory);
        tools.insert("test", crate::tools::create_test_tool as ToolFactory);
        tools.insert("build", crate::tools::create_build_tool as ToolFactory);
        tools.insert(
            "type_check",
            crate::tools::create_type_check_tool as ToolFactory,
        );
        tools.insert("format", crate::tools::create_format_tool as ToolFactory);

        // Legacy echo tool
        tools.insert("echo", crate::tools::create_echo_tool as ToolFactory);

        ToolRegistry {
            tools: Mutex::new(tools),
        }
    }

    /// Create an instance of a tool by name
    pub fn create_tool(&self, name: &str) -> Option<Box<dyn Tool>> {
        self.tools
            .lock()
            .unwrap()
            .get(name)
            .map(|factory| factory())
    }

    /// List all available tool names
    pub fn list_tools(&self) -> Vec<&'static str> {
        let tools = self.tools.lock().unwrap();
        let mut names: Vec<_> = tools.keys().copied().collect();
        names.sort();
        names
    }

    /// Create all available tools
    pub fn create_all_tools(&self) -> Vec<Box<dyn Tool>> {
        let tools = self.tools.lock().unwrap();
        let mut instances: Vec<Box<dyn Tool>> = Vec::new();
        for factory in tools.values() {
            instances.push(factory());
        }
        instances
    }

    /// Check if a tool exists
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.lock().unwrap().contains_key(name)
    }

    /// Get count of registered tools
    pub fn tool_count(&self) -> usize {
        self.tools.lock().unwrap().len()
    }
}

/// Create a tool from the global registry
pub fn get_tool(name: &str) -> Option<Box<dyn Tool>> {
    TOOL_REGISTRY.create_tool(name)
}

/// List all available tool names
pub fn list_all_tools() -> Vec<&'static str> {
    TOOL_REGISTRY.list_tools()
}

/// Create all available tools
pub fn create_all_tools() -> Vec<Box<dyn Tool>> {
    TOOL_REGISTRY.create_all_tools()
}

/// Get total count of available tools
pub fn tool_count() -> usize {
    TOOL_REGISTRY.tool_count()
}
