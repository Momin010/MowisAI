//! Layer 6 isolated test harness — no Gemini, real agentd socket.
//!
//! Run with:
//!   sudo ./target/debug/agentd socket --path /tmp/agentd.sock   # terminal 1
//!   cargo run -- verify-layer6 --socket /tmp/agentd.sock         # terminal 2
//!
//! This exercises the full Layer 6 code path (create containers, run tool
//! calls, apply diffs, destroy containers) without any LLM calls.
//! The test planner returns canned plans, so the only external dependency
//! is the agentd socket.
//!
//! Every step is printed verbosely.  If the real orchestration fails but the
//! simulation succeeds, run this command to isolate whether the failure is in
//! the socket/container layer or in the LLM integration.

use super::agent_execution::AgentExecutor;
use super::sandbox_topology::TopologyManager;
use super::verification::{VerificationLoop, VerificationPlan, VerificationResult};
use agentd_protocol::{SandboxConfig, SandboxName, Task, TaskGraph, TaskId, VerificationStatus};
use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

/// Run Layer 6 verification in isolation with mock LLM responses.
#[derive(Parser, Debug)]
pub struct VerifyLayer6Command {
    /// Path to agentd socket
    #[arg(long, default_value = "/tmp/agentd.sock")]
    pub socket: String,

    /// GCP project ID (only needed if --use-real-llm is set)
    #[arg(long, default_value = "test-project")]
    pub project_id: String,

    /// Project root directory (must exist)
    #[arg(long, default_value = "/tmp/mock-project")]
    pub project_root: PathBuf,

    /// Per-task execution timeout in seconds
    #[arg(long, default_value = "30")]
    pub task_timeout: u64,

    /// Maximum verification rounds
    #[arg(long, default_value = "2")]
    pub max_rounds: usize,

    /// Use a real Gemini call instead of mock plans (requires gcloud auth)
    #[arg(long, default_value = "false")]
    pub use_real_llm: bool,

    /// Simulate a test failure on round 1 to exercise the fix-task path
    #[arg(long, default_value = "false")]
    pub inject_failure: bool,
}

impl VerifyLayer6Command {
    pub async fn run(&self) -> Result<()> {
        println!("\n╔══════════════════════════════════════════════════════════╗");
        println!("║      Layer 6 Isolated Test Harness                      ║");
        println!("╚══════════════════════════════════════════════════════════╝\n");

        println!("Config:");
        println!("  socket:       {}", self.socket);
        println!("  project_root: {}", self.project_root.display());
        println!("  max_rounds:   {}", self.max_rounds);
        println!("  task_timeout: {}s", self.task_timeout);
        println!("  inject_fail:  {}", self.inject_failure);
        println!("  real_llm:     {}\n", self.use_real_llm);

        // ── Step 0: pre-flight checks ──────────────────────────────────────
        println!("[ Step 0 ] Pre-flight checks");

        if !self.project_root.exists() {
            std::fs::create_dir_all(&self.project_root)
                .with_context(|| format!("Cannot create project root {:?}", self.project_root))?;
            println!("  ✓ Created project root: {}", self.project_root.display());
        } else {
            println!("  ✓ Project root exists: {}", self.project_root.display());
        }

        // Probe the socket before doing anything else so the user gets an
        // immediate clear error instead of a cryptic timeout later.
        let probe = super::socket_roundtrip(
            &self.socket,
            &serde_json::json!({ "request_type": "list_sandboxes" }),
        );
        match probe {
            Ok(_) => println!("  ✓ Socket responsive: {}", self.socket),
            Err(e) => {
                println!("  ✗ Socket NOT responsive: {}", e);
                println!();
                println!("  Make sure agentd is running:");
                println!("    sudo ./target/debug/agentd socket --path {}", self.socket);
                return Err(anyhow::anyhow!("Socket not available"));
            }
        }

        // ── Step 1: create sandbox ─────────────────────────────────────────
        println!("\n[ Step 1 ] Creating sandbox");

        let topology = TopologyManager::new(self.project_root.clone(), self.socket.clone())?;
        let sandbox_name: SandboxName = "verify-test-sandbox".to_string();

        let sandbox_config = SandboxConfig {
            name: sandbox_name.clone(),
            scope: "/".to_string(),
            tools: vec![
                "read_file".to_string(),
                "write_file".to_string(),
                "run_command".to_string(),
                "list_files".to_string(),
            ],
            max_agents: 4,
        };

        topology
            .create_sandbox_layer(&sandbox_config)
            .await
            .context("Failed to create sandbox layer")?;
        println!("  ✓ Sandbox created: {}", sandbox_name);

        // ── Step 2: write a tiny project into the sandbox ──────────────────
        println!("\n[ Step 2 ] Seeding project files into sandbox");

        {
            let seed_agent = topology
                .create_agent_layer(&sandbox_name, Some("seed".to_string()))
                .await
                .context("Failed to create seed agent layer")?;
            println!("  ✓ Seed agent container: {}", &seed_agent.container_id[..8]);

            let write_req = serde_json::json!({
                "request_type": "invoke_tool",
                "sandbox": seed_agent.sandbox_name,
                "container": seed_agent.container_id,
                "name": "write_file",
                "input": {
                    "path": "/workspace/add.py",
                    "content": "def add(a, b):\n    return a + b\n"
                }
            });
            let resp = super::socket_roundtrip(&self.socket, &write_req)
                .context("write_file /workspace/add.py failed")?;
            println!("  ✓ Wrote /workspace/add.py — response: {}", resp.get("status").and_then(|s| s.as_str()).unwrap_or("?"));

            let test_req = serde_json::json!({
                "request_type": "invoke_tool",
                "sandbox": seed_agent.sandbox_name,
                "container": seed_agent.container_id,
                "name": "write_file",
                "input": {
                    "path": "/workspace/test_add.py",
                    "content": "from add import add\ndef test_add():\n    assert add(1, 2) == 3\n    assert add(-1, 1) == 0\nprint('tests passed')\n"
                }
            });
            let resp = super::socket_roundtrip(&self.socket, &test_req)
                .context("write_file /workspace/test_add.py failed")?;
            println!("  ✓ Wrote /workspace/test_add.py — response: {}", resp.get("status").and_then(|s| s.as_str()).unwrap_or("?"));

            topology
                .destroy_agent_layer(&seed_agent.agent_id)
                .await
                .context("Failed to destroy seed agent")?;
            println!("  ✓ Seed agent destroyed");
        }

        // ── Step 3: build agent executor ──────────────────────────────────
        println!("\n[ Step 3 ] Building AgentExecutor");

        let checkpoint_root = std::env::temp_dir().join("verify-layer6-checkpoints");
        std::fs::create_dir_all(&checkpoint_root)?;
        let agent_executor = AgentExecutor::new(
            self.project_id.clone(),
            self.socket.clone(),
            checkpoint_root,
        )
        .context("Failed to create AgentExecutor")?;
        println!("  ✓ AgentExecutor ready");

        // ── Step 4: build verification loop ───────────────────────────────
        println!("\n[ Step 4 ] Building VerificationLoop");

        let verification_loop =
            VerificationLoop::new(self.project_id.clone(), self.max_rounds)
                .with_test_timeout(self.task_timeout);

        let original_tasks = vec![Task {
            id: "impl-add".to_string(),
            description: "implement add function in add.py".to_string(),
            deps: vec![],
            hint: Some(sandbox_name.clone()),
        }];

        // Minimal synthetic diff to represent what Layer 5 would produce.
        let merged_diff = r#"diff --git a/add.py b/add.py
--- a/add.py
+++ b/add.py
@@ -0,0 +1,2 @@
+def add(a, b):
+    return a + b
"#;

        println!("  ✓ Verification loop ready (max_rounds={}, timeout={}s)", self.max_rounds, self.task_timeout);

        // ── Step 5: run verify_sandbox ────────────────────────────────────
        println!("\n[ Step 5 ] Running verify_sandbox");
        println!("  (All [VERIFY] log lines below come from the real verification code path)\n");
        println!("─────────────────────────────────────────────────────────────────────");

        let start = std::time::Instant::now();
        let result = if self.use_real_llm {
            // Real path: calls Gemini for test-task generation
            println!("  ⚠  Using real Gemini API — requires gcloud auth");
            verification_loop
                .verify_sandbox(
                    &sandbox_name,
                    merged_diff,
                    &original_tasks,
                    &topology,
                    &agent_executor,
                )
                .await
        } else {
            // Mock path: injects pre-canned plans directly
            run_with_mock_plans(
                &verification_loop,
                &sandbox_name,
                merged_diff,
                &original_tasks,
                &topology,
                &agent_executor,
                self.task_timeout,
                self.inject_failure,
            )
            .await
        };

        let elapsed = start.elapsed();
        println!("─────────────────────────────────────────────────────────────────────\n");

        // ── Step 6: report ────────────────────────────────────────────────
        println!("[ Step 6 ] Result");
        match result {
            Ok(vr) => {
                println!("  Status:          {:?}", vr.status);
                println!("  Rounds:          {}", vr.rounds_completed);
                println!("  Passed tests:    {:?}", vr.passed_tests);
                println!("  Failed tests:    {:?}", vr.failed_tests);
                println!("  Elapsed:         {:?}", elapsed);

                match vr.status {
                    VerificationStatus::Passed => {
                        println!("\n  ✅  Layer 6 PASSED — the full code path works correctly.");
                    }
                    VerificationStatus::PartiallyVerified => {
                        println!("\n  ⚠️   Layer 6 PARTIALLY_VERIFIED — max rounds exhausted with some failures.");
                        println!("  Check the [VERIFY] log lines above for which test tasks failed and why.");
                    }
                    VerificationStatus::Failed => {
                        println!("\n  ❌  Layer 6 FAILED — all tests failed before max rounds.");
                        println!("  Check the [VERIFY] log lines above for error details.");
                    }
                    VerificationStatus::NotStarted => {
                        println!("\n  ⚠️   No tests were executed — check that test task generation worked.");
                    }
                    VerificationStatus::Running => {}
                }
            }
            Err(e) => {
                println!("  ❌  verify_sandbox returned an error:");
                // Print the full error chain — this is what the real orchestration
                // swallows silently.
                let mut source: &dyn std::error::Error = &*e;
                println!("    {}", e);
                while let Some(cause) = source.source() {
                    println!("    caused by: {}", cause);
                    source = cause;
                }
            }
        }

        // ── Cleanup ───────────────────────────────────────────────────────
        println!("\n[ Cleanup ] Destroying sandbox");
        match topology.destroy_sandbox_layer(&sandbox_name).await {
            Ok(_) => println!("  ✓ Sandbox destroyed"),
            Err(e) => println!("  ⚠  Sandbox cleanup failed (not fatal): {}", e),
        }

        println!();
        Ok(())
    }
}

/// Run the verification loop by injecting mock plans directly, bypassing the
/// Gemini API entirely.  Mirrors the internals of `VerificationLoop::verify_sandbox`
/// but uses canned `VerificationPlan` values instead of LLM calls.
async fn run_with_mock_plans(
    vloop: &VerificationLoop,
    sandbox_name: &SandboxName,
    _merged_diff: &str,
    _original_tasks: &[Task],
    topology: &TopologyManager,
    executor: &AgentExecutor,
    task_timeout_secs: u64,
    inject_failure: bool,
) -> Result<VerificationResult> {
    use super::verification::determine_status_for_test;

    let max_rounds = vloop.max_rounds();
    let mut passed_tests: Vec<TaskId> = Vec::new();
    let mut failed_tests: Vec<TaskId> = Vec::new();
    let mut rounds_completed = 0;

    println!("[MOCK] Starting mock verification — {} round(s) max", max_rounds);

    for round in 0..max_rounds {
        rounds_completed = round + 1;
        passed_tests.clear();
        failed_tests.clear();

        // Build canned test plan — round 1 optionally injects a failing task.
        let mut test_tasks = vec![
            Task {
                id: "mock-test-1".to_string(),
                description: "run python test_add.py to verify the add function".to_string(),
                deps: vec![],
                hint: None,
            },
        ];

        if inject_failure && round == 0 {
            test_tasks.push(Task {
                id: "mock-test-fail".to_string(),
                description: "run a command that will fail to test fix-task injection".to_string(),
                deps: vec![],
                hint: None,
            });
        }

        println!(
            "[MOCK] Round {}/{} — injecting {} test task(s)",
            round + 1,
            max_rounds,
            test_tasks.len()
        );

        let plan = VerificationPlan {
            test_tasks: TaskGraph { tasks: test_tasks },
            sandbox_name: sandbox_name.clone(),
        };

        let test_tools = vec![
            "run_command".to_string(),
            "read_file".to_string(),
            "list_files".to_string(),
        ];

        let mut round_failures: Vec<(TaskId, String, String)> = Vec::new();

        for test_task in &plan.test_tasks.tasks {
            println!("[MOCK] Creating agent for test task: {}", test_task.id);

            let agent = match topology
                .create_agent_layer(sandbox_name, Some(test_task.id.clone()))
                .await
            {
                Ok(a) => {
                    println!("[MOCK]   ✓ Agent {} created (container {})", &a.agent_id[..8], &a.container_id[..8]);
                    a
                }
                Err(e) => {
                    println!("[MOCK]   ✗ create_agent_layer FAILED: {}", e);
                    failed_tests.push(test_task.id.clone());
                    round_failures.push((test_task.id.clone(), test_task.description.clone(), e.to_string()));
                    continue;
                }
            };

            let test_prompt = format!(
                "You are a test verification agent. Run this test:\n{}\n\n\
                Use run_command to execute. The working directory is /workspace.\n\
                Report pass or fail.",
                test_task.description
            );

            println!("[MOCK]   Executing task (timeout={}s)...", task_timeout_secs);

            let result = match tokio::time::timeout(
                std::time::Duration::from_secs(task_timeout_secs),
                executor.execute_task(&agent, &test_task.description, &test_tools, &test_prompt),
            )
            .await
            {
                Ok(r) => r,
                Err(_) => {
                    println!("[MOCK]   ✗ TIMEOUT after {}s", task_timeout_secs);
                    Err(anyhow::anyhow!("timeout after {}s", task_timeout_secs))
                }
            };

            let _ = topology.destroy_agent_layer(&agent.agent_id).await;

            match result {
                Ok(r) if r.success => {
                    println!("[MOCK]   ✓ Task {} PASSED", test_task.id);
                    passed_tests.push(test_task.id.clone());
                }
                Ok(r) => {
                    let err = r.error.unwrap_or_else(|| "no error message".to_string());
                    println!("[MOCK]   ✗ Task {} FAILED: {}", test_task.id, err);
                    failed_tests.push(test_task.id.clone());
                    round_failures.push((test_task.id.clone(), test_task.description.clone(), err));
                }
                Err(e) => {
                    println!("[MOCK]   ✗ Task {} ERROR: {}", test_task.id, e);
                    failed_tests.push(test_task.id.clone());
                    round_failures.push((test_task.id.clone(), test_task.description.clone(), e.to_string()));
                }
            }
        }

        if round_failures.is_empty() {
            println!("[MOCK] Round {} — all tests passed ✓", round + 1);
            break;
        }

        println!("[MOCK] Round {} — {} failure(s):", round + 1, round_failures.len());
        for (id, _desc, err) in &round_failures {
            println!("[MOCK]   {} → {}", id, err);
        }

        // On non-final rounds, inject a mock fix task and apply its diff.
        if round < max_rounds - 1 {
            println!("[MOCK] Injecting fix task for round {} failures...", round + 1);

            let fix_task = Task {
                id: "mock-fix-1".to_string(),
                description: "create a simple echo script as a no-op fix".to_string(),
                deps: vec![],
                hint: None,
            };

            let fix_tools = vec![
                "write_file".to_string(),
                "run_command".to_string(),
            ];

            let fix_agent = match topology
                .create_agent_layer(sandbox_name, Some(fix_task.id.clone()))
                .await
            {
                Ok(a) => {
                    println!("[MOCK]   ✓ Fix agent {} created", &a.agent_id[..8]);
                    a
                }
                Err(e) => {
                    println!("[MOCK]   ✗ Fix agent creation FAILED: {}", e);
                    continue;
                }
            };

            let fix_prompt = format!(
                "Write a file /workspace/fix_marker.txt with the text 'fixed'.\n\
                Task: {}",
                fix_task.description
            );

            let fix_result = match tokio::time::timeout(
                std::time::Duration::from_secs(task_timeout_secs),
                executor.execute_task(&fix_agent, &fix_task.description, &fix_tools, &fix_prompt),
            )
            .await
            {
                Ok(r) => r,
                Err(_) => {
                    println!("[MOCK]   ✗ Fix task TIMEOUT after {}s", task_timeout_secs);
                    Err(anyhow::anyhow!("fix timeout"))
                }
            };

            match fix_result {
                Ok(r) if r.success => {
                    println!("[MOCK]   ✓ Fix task succeeded");
                    if let Some(ref diff) = r.git_diff {
                        if !diff.is_empty() {
                            match topology.apply_diff_to_sandbox(sandbox_name, diff).await {
                                Ok(_) => println!("[MOCK]   ✓ Fix diff applied to sandbox ({} bytes)", diff.len()),
                                Err(e) => println!("[MOCK]   ✗ Apply diff FAILED: {}", e),
                            }
                        } else {
                            println!("[MOCK]   ⚠  Fix task produced empty diff");
                        }
                    }
                }
                Ok(r) => println!("[MOCK]   ✗ Fix task did not succeed: {:?}", r.error),
                Err(e) => println!("[MOCK]   ✗ Fix task error: {}", e),
            }

            let _ = topology.destroy_agent_layer(&fix_agent.agent_id).await;
        }
    }

    let status = determine_status_for_test(&failed_tests, &passed_tests, rounds_completed, max_rounds);

    Ok(VerificationResult {
        status,
        passed_tests,
        failed_tests,
        rounds_completed,
    })
}
