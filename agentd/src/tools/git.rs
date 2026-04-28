use crate::tools::common::{resolve_path, Tool, ToolContext};
use serde_json::{json, Value};
use std::process::Command;

pub struct GitCloneTool;
impl Tool for GitCloneTool {
    fn name(&self) -> &'static str {
        "git_clone"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let repo = input["repo"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_clone: missing repo"))?;
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_clone: missing path"))?;

        let output = if let Some(root) = &ctx.root_path {
            Command::new("chroot")
                .arg(root)
                .arg("git")
                .arg("clone")
                .arg(repo)
                .arg(path_str)
                .env("GIT_AUTHOR_NAME", "agentd")
                .env("GIT_AUTHOR_EMAIL", "agentd@mowisai.com")
                .env("GIT_COMMITTER_NAME", "agentd")
                .env("GIT_COMMITTER_EMAIL", "agentd@mowisai.com")
                .output()?
        } else {
            let path = resolve_path(ctx, path_str);
            Command::new("git")
                .arg("clone")
                .arg(repo)
                .arg(&path)
                .env("GIT_AUTHOR_NAME", "agentd")
                .env("GIT_AUTHOR_EMAIL", "agentd@mowisai.com")
                .env("GIT_COMMITTER_NAME", "agentd")
                .env("GIT_COMMITTER_EMAIL", "agentd@mowisai.com")
                .output()?
        };

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(GitCloneTool)
    }
}

pub struct GitStatusTool;
impl Tool for GitStatusTool {
    fn name(&self) -> &'static str {
        "git_status"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_status: missing path"))?;

        let path = resolve_path(ctx, path_str);

        let output = Command::new("git")
            .arg("-C")
            .arg(&path)
            .arg("status")
            .output()?;

        Ok(json!({
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "success": output.status.success()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(GitStatusTool)
    }
}

pub struct GitAddTool;
impl Tool for GitAddTool {
    fn name(&self) -> &'static str {
        "git_add"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_add: missing path"))?;
        let files = input["files"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("git_add: missing files"))?;

        let file_args: Vec<&str> = files.iter().filter_map(|v| v.as_str()).collect();

        let output = if let Some(root) = &ctx.root_path {
            let mut cmd = Command::new("chroot");
            cmd.arg(root)
                .arg("git")
                .arg("-C")
                .arg(path_str)
                .arg("add");
            for file in &file_args {
                cmd.arg(file);
            }
            cmd.env("GIT_AUTHOR_NAME", "agentd")
                .env("GIT_AUTHOR_EMAIL", "agentd@mowisai.com")
                .env("GIT_COMMITTER_NAME", "agentd")
                .env("GIT_COMMITTER_EMAIL", "agentd@mowisai.com")
                .output()?
        } else {
            let path = resolve_path(ctx, path_str);
            let mut cmd = Command::new("git");
            cmd.arg("-C").arg(&path).arg("add");
            for file in &file_args {
                cmd.arg(file);
            }
            cmd.env("GIT_AUTHOR_NAME", "agentd")
                .env("GIT_AUTHOR_EMAIL", "agentd@mowisai.com")
                .env("GIT_COMMITTER_NAME", "agentd")
                .env("GIT_COMMITTER_EMAIL", "agentd@mowisai.com")
                .output()?
        };

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(GitAddTool)
    }
}

pub struct GitCommitTool;
impl Tool for GitCommitTool {
    fn name(&self) -> &'static str {
        "git_commit"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_commit: missing path"))?;
        let message = input["message"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_commit: missing message"))?;

        let output = if let Some(root) = &ctx.root_path {
            let mut cmd = Command::new("chroot");
            cmd.arg(root)
                .arg("git")
                .arg("-C")
                .arg(path_str)
                .arg("commit")
                .arg("-m")
                .arg(message);
            cmd.env("GIT_AUTHOR_NAME", "agentd")
                .env("GIT_AUTHOR_EMAIL", "agentd@mowisai.com")
                .env("GIT_COMMITTER_NAME", "agentd")
                .env("GIT_COMMITTER_EMAIL", "agentd@mowisai.com")
                .output()?
        } else {
            let path = resolve_path(ctx, path_str);
            let mut cmd = Command::new("git");
            cmd.arg("-C")
                .arg(&path)
                .arg("commit")
                .arg("-m")
                .arg(message);
            cmd.env("GIT_AUTHOR_NAME", "agentd")
                .env("GIT_AUTHOR_EMAIL", "agentd@mowisai.com")
                .env("GIT_COMMITTER_NAME", "agentd")
                .env("GIT_COMMITTER_EMAIL", "agentd@mowisai.com")
                .output()?
        };

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(GitCommitTool)
    }
}

pub struct GitPushTool;
impl Tool for GitPushTool {
    fn name(&self) -> &'static str {
        "git_push"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_push: missing path"))?;
        let remote = input["remote"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_push: missing remote"))?;
        let branch = input["branch"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_push: missing branch"))?;

        let path = resolve_path(ctx, path_str);

        let output = Command::new("git")
            .arg("-C")
            .arg(&path)
            .arg("push")
            .arg(remote)
            .arg(branch)
            .output()?;

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(GitPushTool)
    }
}

pub struct GitPullTool;
impl Tool for GitPullTool {
    fn name(&self) -> &'static str {
        "git_pull"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_pull: missing path"))?;
        let remote = input["remote"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_pull: missing remote"))?;
        let branch = input["branch"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_pull: missing branch"))?;

        let path = resolve_path(ctx, path_str);

        let output = Command::new("git")
            .arg("-C")
            .arg(&path)
            .arg("pull")
            .arg(remote)
            .arg(branch)
            .output()?;

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(GitPullTool)
    }
}

pub struct GitBranchTool;
impl Tool for GitBranchTool {
    fn name(&self) -> &'static str {
        "git_branch"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_branch: missing path"))?;
        let name = input.get("name").and_then(|v| v.as_str());

        let path = resolve_path(ctx, path_str);

        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(&path).arg("branch");
        if let Some(n) = name {
            cmd.arg(n);
        }

        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let branches: Vec<String> = stdout.lines().map(|l| l.to_string()).collect();

        Ok(json!({
            "branches": branches,
            "success": output.status.success()
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(GitBranchTool)
    }
}

pub struct GitCheckoutTool;
impl Tool for GitCheckoutTool {
    fn name(&self) -> &'static str {
        "git_checkout"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_checkout: missing path"))?;
        let branch = input["branch"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_checkout: missing branch"))?;

        if let Some(root) = &ctx.root_path {
            // chroot branch: we accept an optional `create` flag, defaulting to true
            let create = input.get("create").and_then(|v| v.as_bool()).unwrap_or(true);
            let mut cmd = Command::new("chroot");
            cmd.arg(root)
                .arg("git")
                .arg("-C")
                .arg(path_str)
                .arg("checkout");
            if create {
                cmd.arg("-b");
            }
            cmd.arg(branch);
            // use explicit identity for chrooted operations (pattern from spec)
            cmd.env("GIT_AUTHOR_NAME", "Momin")
                .env("GIT_AUTHOR_EMAIL", "momin.aldahdooh@gmail.com")
                .env("GIT_COMMITTER_NAME", "Momin")
                .env("GIT_COMMITTER_EMAIL", "momin.aldahdooh@gmail.com");

            let output = cmd.output()?;
            return Ok(json!({
                "success": output.status.success(),
                "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            }));
        } else {
            let path = resolve_path(ctx, path_str);

            // Try with -b flag first (create and checkout)
            let mut cmd = Command::new("git");
            cmd.arg("-C")
                .arg(&path)
                .arg("checkout")
                .arg("-b")
                .arg(branch);
            cmd.env("GIT_AUTHOR_NAME", "agentd")
                .env("GIT_AUTHOR_EMAIL", "agentd@mowisai.com")
                .env("GIT_COMMITTER_NAME", "agentd")
                .env("GIT_COMMITTER_EMAIL", "agentd@mowisai.com");

            let output = cmd.output()?;

            if !output.status.success() {
                // Try without -b flag (branch might already exist)
                let mut cmd2 = Command::new("git");
                cmd2.arg("-C")
                    .arg(&path)
                    .arg("checkout")
                    .arg(branch);
                cmd2.env("GIT_AUTHOR_NAME", "agentd")
                    .env("GIT_AUTHOR_EMAIL", "agentd@mowisai.com")
                    .env("GIT_COMMITTER_NAME", "agentd")
                    .env("GIT_COMMITTER_EMAIL", "agentd@mowisai.com");

                let output2 = cmd2.output()?;
                return Ok(json!({
                    "success": output2.status.success(),
                    "stdout": String::from_utf8_lossy(&output2.stdout).to_string(),
                    "stderr": String::from_utf8_lossy(&output2.stderr).to_string(),
                }));
            }

            Ok(json!({
                "success": true,
                "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
                "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            }))
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(GitCheckoutTool)
    }
}

pub struct GitDiffTool;
impl Tool for GitDiffTool {
    fn name(&self) -> &'static str {
        "git_diff"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("git_diff: missing path"))?;
        let staged = input.get("staged").and_then(|v| v.as_bool()).unwrap_or(false);
        let diff_arg = if staged { "diff --cached" } else { "diff" };

        let output = if let Some(root) = &ctx.root_path {
            let mut cmd = Command::new("chroot");
            cmd.arg(root)
                .arg("git")
                .arg("-C")
                .arg(path_str);
            // split diff_arg into two parts if it contains a space
            for part in diff_arg.split_whitespace() {
                cmd.arg(part);
            }
            cmd.output()?
        } else {
            let path = resolve_path(ctx, path_str);
            let mut cmd = Command::new("git");
            cmd.arg("-C").arg(&path);
            for part in diff_arg.split_whitespace() {
                cmd.arg(part);
            }
            cmd.output()?
        };

        Ok(json!({
            "diff": String::from_utf8_lossy(&output.stdout).to_string(),
            "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
            "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            "success": output.status.success(),
        }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(GitDiffTool)
    }
}
