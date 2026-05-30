use crate::providers::ToolCall;

#[derive(Debug, Clone)]
pub enum ToolOutcome {
    Ok(serde_json::Value),
    Err(String),
    Denied,
}

impl ToolOutcome {
    pub fn is_ok(&self) -> bool {
        matches!(self, ToolOutcome::Ok(_))
    }

    pub fn ok_value(&self) -> Option<&serde_json::Value> {
        match self {
            ToolOutcome::Ok(v) => Some(v),
            _ => None,
        }
    }
}

impl From<serde_json::Value> for ToolOutcome {
    fn from(v: serde_json::Value) -> Self {
        if let Some(err) = v.get("error") {
            if let Some(msg) = err.as_str() {
                return ToolOutcome::Err(msg.to_string());
            }
        }
        ToolOutcome::Ok(v)
    }
}

pub fn summarize(call: &ToolCall, outcome: &ToolOutcome) -> String {
    let success = matches!(outcome, ToolOutcome::Ok(_));
    let err_tail = match outcome {
        ToolOutcome::Err(e) => format!(" (FAILED: {})", truncate(e, 60)),
        ToolOutcome::Denied => " (DENIED by tool whitelist)".to_string(),
        ToolOutcome::Ok(_) => "".to_string(),
    };

    match call.name.as_str() {
        "run_command" | "shell_exec" => {
            let cmd = call
                .args
                .get("cmd")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            format!(
                "Agent ran a terminal command: `{}`{}",
                truncate(cmd, 80),
                err_tail
            )
        }
        "read_file" => {
            let path = call
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            let size = outcome
                .ok_value()
                .and_then(|v| v.get("size"))
                .and_then(|v| v.as_u64())
                .map(human_bytes)
                .unwrap_or_else(|| "?".into());
            if success {
                format!("Agent read {} ({})", path, size)
            } else {
                format!("Agent tried to read {}{}", path, err_tail)
            }
        }
        "write_file" => {
            let path = call
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            let lines = call
                .args
                .get("contents")
                .or_else(|| call.args.get("content"))
                .and_then(|v| v.as_str())
                .map(|s| s.lines().count())
                .unwrap_or(0);
            if success {
                format!("Agent wrote {} lines to {}", lines, path)
            } else {
                format!("Agent tried to write to {}{}", path, err_tail)
            }
        }
        "append_file" => {
            let path = call
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            if success {
                format!("Agent appended to {}", path)
            } else {
                format!("Agent tried to append to {}{}", path, err_tail)
            }
        }
        "list_files" | "list_dir" => {
            let path = call
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let count = outcome
                .ok_value()
                .and_then(|v| v.get("entries"))
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            if success {
                format!("Agent listed {} entries in {}", count, path)
            } else {
                format!("Agent tried to list {}{}", path, err_tail)
            }
        }
        "grep" | "find_files" => {
            let pattern = call
                .args
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            let path = call
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let matches = outcome
                .ok_value()
                .and_then(|v| v.get("count"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if success {
                format!(
                    "Agent searched for `{}` in {} ({} matches)",
                    truncate(pattern, 40),
                    path,
                    matches
                )
            } else {
                format!("Agent tried to search for `{}`{}", pattern, err_tail)
            }
        }
        "git_commit" => {
            let msg = call
                .args
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let files = outcome
                .ok_value()
                .and_then(|v| v.get("files_changed"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if success {
                format!("Agent committed: '{}' ({} files)", truncate(msg, 50), files)
            } else {
                format!("Agent tried to commit{}", err_tail)
            }
        }
        "git_diff" => {
            let path = call
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("(repo)");
            format!("Agent diffed {}{}", path, err_tail)
        }
        "git_status" => {
            format!("Agent checked git status{}", err_tail)
        }
        "git_add" => {
            format!("Agent staged files{}", err_tail)
        }
        "http_get" | "http_post" | "http_put" | "http_delete" | "http_patch" => {
            let url = call
                .args
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            let method = call
                .name
                .strip_prefix("http_")
                .unwrap()
                .to_uppercase();
            let status = outcome
                .ok_value()
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_u64())
                .map(|s| format!(" ({})", s))
                .unwrap_or_default();
            if success {
                format!(
                    "Agent fetched {} {}{}",
                    method,
                    truncate(url, 60),
                    status
                )
            } else {
                format!(
                    "Agent tried to fetch {} {}{}",
                    method, url, err_tail
                )
            }
        }
        "docker_run" => {
            let image = call
                .args
                .get("image")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            let cmd = call
                .args
                .get("cmd")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!(
                "Agent ran docker container: {} {}{}",
                image,
                truncate(cmd, 40),
                err_tail
            )
        }
        "apply_patch" => {
            let path = call
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            if success {
                format!("Agent applied patch to {}", path)
            } else {
                format!("Agent failed to apply patch to {}{}", path, err_tail)
            }
        }
        "delete_file" => {
            let path = call
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            if success {
                format!("Agent deleted {}", path)
            } else {
                format!("Agent tried to delete {}{}", path, err_tail)
            }
        }
        "create_directory" => {
            let path = call
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            if success {
                format!("Agent created directory {}", path)
            } else {
                format!("Agent tried to create directory {}{}", path, err_tail)
            }
        }
        "copy_file" => {
            let from = call
                .args
                .get("from")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            let to = call
                .args
                .get("to")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            if success {
                format!("Agent copied {} to {}", from, to)
            } else {
                format!("Agent tried to copy {} to {}{}", from, to, err_tail)
            }
        }
        "run_script" => {
            let path = call
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("<inline>");
            if success {
                format!("Agent ran script {}", path)
            } else {
                format!("Agent script {} failed{}", path, err_tail)
            }
        }
        other => format!("Agent invoked `{}`{}", other, err_tail),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}...", &s[..n])
    }
}

fn human_bytes(n: u64) -> String {
    if n < 1024 {
        format!("{} B", n)
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else if n < 1024 * 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", n as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "test-1".into(),
            name: name.into(),
            args,
        }
    }

    #[test]
    fn test_summarize_shell_exec_success() {
        let c = call("run_command", json!({"cmd": "npm install"}));
        let o = ToolOutcome::Ok(json!({"exit_code": 0, "success": true}));
        assert_eq!(
            summarize(&c, &o),
            "Agent ran a terminal command: `npm install`"
        );
    }

    #[test]
    fn test_summarize_shell_exec_failure() {
        let c = call("run_command", json!({"cmd": "npm install"}));
        let o = ToolOutcome::Err("exit code 1".into());
        assert_eq!(
            summarize(&c, &o),
            "Agent ran a terminal command: `npm install` (FAILED: exit code 1)"
        );
    }

    #[test]
    fn test_summarize_read_file_success() {
        let c = call("read_file", json!({"path": "src/main.rs"}));
        let o = ToolOutcome::Ok(json!({"content": "fn main() {}", "size": 4200, "success": true}));
        assert_eq!(summarize(&c, &o), "Agent read src/main.rs (4.1 KB)");
    }

    #[test]
    fn test_summarize_read_file_failure() {
        let c = call("read_file", json!({"path": "src/main.rs"}));
        let o = ToolOutcome::Err("file not found".into());
        assert_eq!(
            summarize(&c, &o),
            "Agent tried to read src/main.rs (FAILED: file not found)"
        );
    }

    #[test]
    fn test_summarize_write_file_success() {
        let c = call(
            "write_file",
            json!({"path": "src/main.rs", "contents": "line1\nline2\nline3\n"}),
        );
        let o = ToolOutcome::Ok(json!({"path": "src/main.rs", "bytes": 20, "success": true}));
        assert_eq!(summarize(&c, &o), "Agent wrote 3 lines to src/main.rs");
    }

    #[test]
    fn test_summarize_list_dir_success() {
        let c = call("list_dir", json!({"path": "src/"}));
        let o = ToolOutcome::Ok(json!({"entries": [{"name": "main.rs"}, {"name": "lib.rs"}], "success": true}));
        assert_eq!(summarize(&c, &o), "Agent listed 2 entries in src/");
    }

    #[test]
    fn test_summarize_grep_success() {
        let c = call("grep", json!({"pattern": "fn main", "path": "src/"}));
        let o = ToolOutcome::Ok(json!({"matches": ["fn main() {}"], "count": 1, "success": true}));
        assert_eq!(
            summarize(&c, &o),
            "Agent searched for `fn main` in src/ (1 matches)"
        );
    }

    #[test]
    fn test_summarize_git_commit_success() {
        let c = call(
            "git_commit",
            json!({"path": ".", "message": "feat: add /healthz endpoint"}),
        );
        let o = ToolOutcome::Ok(json!({"exit_code": 0, "success": true}));
        assert_eq!(
            summarize(&c, &o),
            "Agent committed: 'feat: add /healthz endpoint' (0 files)"
        );
    }

    #[test]
    fn test_summarize_http_get_success() {
        let c = call("http_get", json!({"url": "https://api.example.com/v1/health"}));
        let o = ToolOutcome::Ok(json!({"status": 200, "body": "ok", "success": true}));
        assert_eq!(
            summarize(&c, &o),
            "Agent fetched GET https://api.example.com/v1/health (200)"
        );
    }

    #[test]
    fn test_summarize_docker_run() {
        let c = call(
            "docker_run",
            json!({"image": "alpine:3.19", "cmd": "sh -c 'echo hello'"}),
        );
        let o = ToolOutcome::Ok(json!({"exit_code": 0, "success": true}));
        assert_eq!(
            summarize(&c, &o),
            "Agent ran docker container: alpine:3.19 sh -c 'echo hello'"
        );
    }

    #[test]
    fn test_summarize_denied() {
        let c = call("shell_exec", json!({"cmd": "rm -rf /"}));
        let o = ToolOutcome::Denied;
        assert_eq!(
            summarize(&c, &o),
            "Agent ran a terminal command: `rm -rf /` (DENIED by tool whitelist)"
        );
    }

    #[test]
    fn test_summarize_unknown_tool() {
        let c = call("custom_tool", json!({"foo": "bar"}));
        let o = ToolOutcome::Ok(json!({"result": "ok"}));
        assert_eq!(summarize(&c, &o), "Agent invoked `custom_tool`");
    }

    #[test]
    fn test_summarize_delete_file() {
        let c = call("delete_file", json!({"path": "old.txt"}));
        let o = ToolOutcome::Ok(json!({"success": true}));
        assert_eq!(summarize(&c, &o), "Agent deleted old.txt");
    }

    #[test]
    fn test_summarize_git_diff() {
        let c = call("git_diff", json!({"path": "src/"}));
        let o = ToolOutcome::Ok(json!({"diff": "..."}));
        assert_eq!(summarize(&c, &o), "Agent diffed src/");
    }

    #[test]
    fn test_human_bytes() {
        assert_eq!(human_bytes(500), "500 B");
        assert_eq!(human_bytes(1500), "1.5 KB");
        assert_eq!(human_bytes(1500000), "1.4 MB");
        assert_eq!(human_bytes(1500000000), "1.4 GB");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello...");
    }
}
