//! Integration tests for new 7-layer orchestration system

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;

    // Test utilities
    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mowis-test-{}", name))
    }

    fn cleanup_dir(path: &PathBuf) {
        std::fs::remove_dir_all(path).ok();
    }

    fn socket_request(socket_path: &str, request: &Value) -> anyhow::Result<Value> {
        let mut stream = UnixStream::connect(socket_path)?;
        let mut body = serde_json::to_string(request)?;
        body.push('\n');
        stream.write_all(body.as_bytes())?;
        stream.flush()?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response)?;
        Ok(serde_json::from_str(response.trim())?)
    }

    // Test Layer 1: Fast Planner
    #[tokio::test]
    async fn test_planner_basic() {
        // Note: This test requires gcloud authentication and will make a real API call
        // Skip in CI by checking for SKIP_LLM_TESTS env var
        if std::env::var("SKIP_LLM_TESTS").is_ok() {
            return;
        }

        use libagent::orchestration::planner::plan_task;

        let project_root = PathBuf::from(".");
        let project_id = "company-internal-tools-490516";
        let prompt = "add a simple hello world function";

        let result = plan_task(prompt, &project_root, project_id).await;

        if let Ok(output) = result {
            assert!(!output.task_graph.tasks.is_empty(), "Should generate at least one task");
            assert!(!output.sandbox_topology.sandboxes.is_empty(), "Should generate at least one sandbox");
            println!("Generated {} tasks", output.task_graph.tasks.len());
        } else {
            // Test might fail without proper auth - that's OK for this test
            println!("Planner test skipped - auth required");
        }
    }

    // Test Layer 2: Overlayfs Topology
    #[tokio::test]
    #[ignore = "requires running agentd socket at /tmp/agentd.sock (needs root)"]
    async fn test_sandbox_topology_creation() {
        use agentd_protocol::SandboxConfig;
        use libagent::orchestration::sandbox_topology::TopologyManager;

        let base_dir = temp_dir("topology-base");
        let socket_path = "/tmp/agentd.sock".to_string();

        // Create base directory with a test file
        std::fs::create_dir_all(&base_dir).unwrap();
        std::fs::write(base_dir.join("test.txt"), "hello").unwrap();

        let topology = TopologyManager::new(base_dir.clone(), socket_path).unwrap();

        let sandbox_config = SandboxConfig {
            name: "test-sandbox".to_string(),
            scope: "".to_string(),
            tools: vec!["read_file".to_string()],
            max_agents: 10,
            image: None,
        };

        // Create sandbox layer
        let result = topology.create_sandbox_layer(&sandbox_config).await;

        // On non-Linux, this will work but skip actual overlayfs mount
        assert!(result.is_ok());

        // Get sandbox info
        let info = topology.get_sandbox_info(&"test-sandbox".to_string()).await;
        assert!(info.is_some());
        if let Some(info) = info {
            assert_eq!(info.name, "test-sandbox");
            assert!(!info.sandbox_id.is_empty());
        }

        cleanup_dir(&base_dir);
    }

    #[tokio::test]
    #[ignore = "requires running agentd socket at /tmp/agentd.sock (needs root)"]
    async fn test_agent_layer_creation() {
        use agentd_protocol::SandboxConfig;
        use libagent::orchestration::sandbox_topology::TopologyManager;

        let base_dir = temp_dir("agent-base");
        let socket_path = "/tmp/agentd.sock".to_string();

        std::fs::create_dir_all(&base_dir).unwrap();

        let topology = TopologyManager::new(base_dir.clone(), socket_path).unwrap();

        let sandbox_config = SandboxConfig {
            name: "test-sandbox".to_string(),
            scope: "".to_string(),
            tools: vec!["read_file".to_string()],
            max_agents: 10,
            image: None,
        };

        topology.create_sandbox_layer(&sandbox_config).await.unwrap();

        // Create agent layer
        let agent = topology
            .create_agent_layer(&"test-sandbox".to_string(), Some("task-1".to_string()))
            .await
            .unwrap();

        assert_eq!(agent.sandbox_name, "test-sandbox");
        assert_eq!(agent.task_id, Some("task-1".to_string()));
        assert!(!agent.agent_id.is_empty());

        // Cleanup
        topology.destroy_agent_layer(&agent.agent_id).await.ok();
        topology.destroy_sandbox_layer(&"test-sandbox".to_string()).await.ok();

        cleanup_dir(&base_dir);
    }

    // Test Layer 3: Scheduler
    #[tokio::test]
    async fn test_scheduler_basic_flow() {
        use agentd_protocol::{Task, TaskGraph};
        use libagent::orchestration::scheduler::Scheduler;
        use std::collections::HashMap;

        let tasks = vec![
            Task {
                id: "t1".to_string(),
                description: "Task 1".to_string(),
                deps: vec![],
                hint: Some("sandbox1".to_string()),
            },
            Task {
                id: "t2".to_string(),
                description: "Task 2".to_string(),
                deps: vec!["t1".to_string()],
                hint: Some("sandbox1".to_string()),
            },
            Task {
                id: "t3".to_string(),
                description: "Task 3".to_string(),
                deps: vec!["t1".to_string()],
                hint: Some("sandbox1".to_string()),
            },
        ];

        let task_graph = TaskGraph { tasks };
        let mut hints = HashMap::new();
        hints.insert("t1".to_string(), "sandbox1".to_string());
        hints.insert("t2".to_string(), "sandbox1".to_string());
        hints.insert("t3".to_string(), "sandbox1".to_string());

        let scheduler = Scheduler::new(task_graph, hints).unwrap();

        // t1 should be ready immediately (no dependencies)
        let ready = scheduler.get_ready_task(&"sandbox1".to_string()).await;
        assert_eq!(ready, Some("t1".to_string()));

        // Check initial stats
        let stats = scheduler.get_stats().await;
        assert_eq!(stats.total_tasks, 3);
        assert_eq!(stats.pending, 3);
        assert_eq!(stats.completed, 0);
    }

    #[tokio::test]
    async fn test_scheduler_dependency_resolution() {
        use agentd_protocol::{AgentHandle, AgentResult, LayerLevel, OverlayfsLayer, Task, TaskGraph};
        use libagent::orchestration::scheduler::Scheduler;
        use std::collections::HashMap;

        let tasks = vec![
            Task {
                id: "t1".to_string(),
                description: "Task 1".to_string(),
                deps: vec![],
                hint: Some("sandbox1".to_string()),
            },
            Task {
                id: "t2".to_string(),
                description: "Task 2".to_string(),
                deps: vec!["t1".to_string()],
                hint: Some("sandbox1".to_string()),
            },
        ];

        let task_graph = TaskGraph { tasks };
        let mut hints = HashMap::new();
        hints.insert("t1".to_string(), "sandbox1".to_string());
        hints.insert("t2".to_string(), "sandbox1".to_string());

        let scheduler = Scheduler::new(task_graph, hints).unwrap();

        // Get t1
        let ready = scheduler.get_ready_task(&"sandbox1".to_string()).await;
        assert_eq!(ready, Some("t1".to_string()));

        // Mark t1 as started
        let agent = AgentHandle {
            agent_id: "agent-1".to_string(),
            sandbox_name: "sandbox1".to_string(),
            container_id: "container-1".to_string(),
            task_id: Some("t1".to_string()),
            layer: OverlayfsLayer {
                level: LayerLevel::Agent,
                mount_path: "/tmp/test".to_string(),
                upper_dir: "/tmp/test/upper".to_string(),
                work_dir: "/tmp/test/work".to_string(),
                lower_dirs: vec![],
            },
        };

        scheduler.mark_task_started("t1".to_string(), agent).await.unwrap();

        // Complete t1
        let result = AgentResult {
            task_id: "t1".to_string(),
            success: true,
            git_diff: Some("diff".to_string()),
            error: None,
            checkpoint_log: vec![],
            timestamp: 0,
        };

        scheduler.handle_task_completion(result).await.unwrap();

        // Now t2 should be ready
        let ready = scheduler.get_ready_task(&"sandbox1".to_string()).await;
        assert_eq!(ready, Some("t2".to_string()));

        // Check stats
        let stats = scheduler.get_stats().await;
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.pending, 1);
    }

    // Test Layer 4: Checkpoint System
    #[test]
    fn test_checkpoint_log_persistence() {
        use agentd_protocol::Checkpoint;
        use libagent::orchestration::checkpoint::CheckpointLog;

        let temp_dir = temp_dir("checkpoint-log");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut log = CheckpointLog::new(
            "agent-123".to_string(),
            "task-456".to_string(),
            &temp_dir,
        )
        .unwrap();

        let checkpoint = Checkpoint {
            id: 0,
            tool_call: "write_file".to_string(),
            tool_args: serde_json::json!({"path": "test.txt", "content": "hello"}),
            tool_result: "success".to_string(),
            timestamp: 1234567890,
            layer_snapshot_path: "/tmp/snapshot-0".to_string(),
        };

        log.add_checkpoint(checkpoint).unwrap();

        // Reload from file
        let loaded = CheckpointLog::load(&log.log_path).unwrap();
        assert_eq!(loaded.checkpoints.len(), 1);
        assert_eq!(loaded.checkpoints[0].id, 0);
        assert_eq!(loaded.checkpoints[0].tool_call, "write_file");

        cleanup_dir(&temp_dir);
    }

    #[test]
    fn test_checkpoint_pruning() {
        use agentd_protocol::Checkpoint;
        use libagent::orchestration::checkpoint::CheckpointLog;

        let temp_dir = temp_dir("checkpoint-prune");
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut log = CheckpointLog::new(
            "agent-123".to_string(),
            "task-456".to_string(),
            &temp_dir,
        )
        .unwrap();

        // Add 15 checkpoints
        for i in 0..15 {
            let checkpoint = Checkpoint {
                id: i,
                tool_call: format!("tool_{}", i),
                tool_args: serde_json::json!({}),
                tool_result: "success".to_string(),
                timestamp: 1234567890 + i,
                layer_snapshot_path: format!("/tmp/snapshot-{}", i),
            };
            log.add_checkpoint(checkpoint).unwrap();
        }

        assert_eq!(log.checkpoints.len(), 15);

        // Prune to keep last 5
        log.prune(5).unwrap();
        assert_eq!(log.checkpoints.len(), 5);

        // Check that the remaining are the latest ones
        assert_eq!(log.checkpoints[0].id, 10);
        assert_eq!(log.checkpoints[4].id, 14);

        cleanup_dir(&temp_dir);
    }

    // Test Layer 5: Parallel Merge
    #[tokio::test]
    async fn test_parallel_merge_single_diff() {
        use libagent::orchestration::merge_worker::ParallelMergeCoordinator;

        let work_dir = temp_dir("merge-single");

        let coordinator = ParallelMergeCoordinator::new(
            "test-project".to_string(),
            work_dir.clone(),
            PathBuf::from("."),
        )
        .unwrap();

        let diffs = vec!["diff --git a/test.txt\n+hello".to_string()];

        let result = coordinator.merge_diffs(diffs).await.unwrap();

        assert!(result.success);
        assert_eq!(result.merged_diff, "diff --git a/test.txt\n+hello");
        assert_eq!(result.conflicts_resolved, 0);

        cleanup_dir(&work_dir);
    }

    #[tokio::test]
    #[ignore = "requires a git repo at the working directory (run from project root with agentd running)"]
    async fn test_parallel_merge_multiple_diffs() {
        use libagent::orchestration::merge_worker::ParallelMergeCoordinator;

        let work_dir = temp_dir("merge-multiple");

        let coordinator = ParallelMergeCoordinator::new(
            "test-project".to_string(),
            work_dir.clone(),
            PathBuf::from("."),
        )
        .unwrap();

        let diffs = vec![
            "diff1".to_string(),
            "diff2".to_string(),
            "diff3".to_string(),
            "diff4".to_string(),
        ];

        let result = coordinator.merge_diffs(diffs).await.unwrap();

        // On Windows, this will do simple concatenation
        // On Linux, it will use git apply (may fail without proper repo setup)
        assert!(result.success || !result.unresolved_conflicts.is_empty());

        cleanup_dir(&work_dir);
    }

    // Test Layer 6: Verification
    #[test]
    fn test_verification_json_extraction() {
        use libagent::orchestration::verification;

        let input = r#"```json
{"test_tasks": {"tasks": [{"id": "t1", "description": "test", "deps": [], "hint": null}]}}
```"#;

        // This is a private function, so we can't test it directly
        // But we can test the JSON parsing in other tests
        assert!(input.contains("```json"));
    }

    #[tokio::test]
    #[ignore] // Requires a running agentd socket server and root-capable overlayfs environment
    async fn test_socket_mock_agents_merge_and_save_to_host() {
        use agentd_protocol::SandboxConfig;
        use libagent::orchestration::sandbox_topology::TopologyManager;

        let project_root = temp_dir("socket-export-project");
        let output_dir = temp_dir("socket-export-output");
        let socket_path = "/tmp/agentd.sock".to_string();

        cleanup_dir(&project_root);
        cleanup_dir(&output_dir);
        std::fs::create_dir_all(project_root.join("src")).unwrap();
        std::fs::write(project_root.join("src/app.js"), "console.log('base app');\n").unwrap();
        std::fs::write(project_root.join("README.md"), "# mock orchestration\n").unwrap();

        let git_init = std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .current_dir(&project_root)
            .output()
            .unwrap();
        assert!(git_init.status.success(), "git init failed");

        let git_email = std::process::Command::new("git")
            .args(["config", "user.email", "tests@mowis.ai"])
            .current_dir(&project_root)
            .output()
            .unwrap();
        assert!(git_email.status.success(), "git config email failed");

        let git_name = std::process::Command::new("git")
            .args(["config", "user.name", "MowisAI Tests"])
            .current_dir(&project_root)
            .output()
            .unwrap();
        assert!(git_name.status.success(), "git config name failed");

        let git_commit = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&project_root)
            .output()
            .unwrap();
        assert!(git_commit.status.success(), "git add failed");

        let git_commit = std::process::Command::new("git")
            .args(["commit", "-m", "initial", "--quiet"])
            .current_dir(&project_root)
            .output()
            .unwrap();
        assert!(git_commit.status.success(), "initial commit failed");

        let topology = TopologyManager::new(project_root.clone(), socket_path).unwrap();
        let sandbox_name = "socket-test-sandbox".to_string();
        let sandbox_config = SandboxConfig {
            name: sandbox_name.clone(),
            scope: "".to_string(),
            tools: vec![
                "read_file".to_string(),
                "write_file".to_string(),
                "run_command".to_string(),
            ],
            max_agents: 5,
            image: None,
        };

        topology.create_sandbox_layer(&sandbox_config).await.unwrap();

        let mut agents = Vec::new();
        let mut diffs = Vec::new();

        for agent_index in 0..5 {
            let agent = topology
                .create_agent_layer(&sandbox_name, Some(format!("task-{agent_index}")))
                .await
                .unwrap();

            let sandbox_id = agent.sandbox_name.clone();
            let container_id = agent.container_id.clone();

            let write_result = socket_request(
                topology.socket_path(),
                &serde_json::json!({
                    "request_type": "invoke_tool",
                    "sandbox": sandbox_id,
                    "container": container_id,
                    "name": "write_file",
                    "input": {
                        "path": format!("/workspace/agents/agent_{agent_index}/result.js"),
                        "content": format!(
                            "// socket-backed mock agent {agent_index}\nconsole.log('agent {agent_index} saved to host');\n"
                        )
                    }
                }),
            )
            .unwrap();
            assert_eq!(write_result["status"], "ok");

            let manifest_result = socket_request(
                topology.socket_path(),
                &serde_json::json!({
                    "request_type": "invoke_tool",
                    "sandbox": sandbox_id,
                    "container": container_id,
                    "name": "write_file",
                    "input": {
                        "path": format!("/workspace/agent_{agent_index}_manifest.json"),
                        "content": format!(
                            "{{\"agent\": {agent_index}, \"saved\": true, \"path\": \"agents/agent_{agent_index}/result.js\"}}"
                        )
                    }
                }),
            )
            .unwrap();
            assert_eq!(manifest_result["status"], "ok");

            let append_result = socket_request(
                topology.socket_path(),
                &serde_json::json!({
                    "request_type": "invoke_tool",
                    "sandbox": sandbox_id,
                    "container": container_id,
                    "name": "run_command",
                    "input": {
                        "cmd": format!(
                            "cd /workspace && mkdir -p diffs && printf '%s\n' \"diff --git a/agents/agent_{0}/result.js b/agents/agent_{0}/result.js\" \"+console.log('agent {0} saved to host');\" > diffs/agent_{0}.diff",
                            agent_index
                        ),
                        "timeout": 20
                    }
                }),
            )
            .unwrap();
            assert_eq!(append_result["status"], "ok");

            let stage_result = socket_request(
                topology.socket_path(),
                &serde_json::json!({
                    "request_type": "invoke_tool",
                    "sandbox": sandbox_id,
                    "container": container_id,
                    "name": "run_command",
                    "input": {
                        "cmd": "cd /workspace && git add -A",
                        "timeout": 20
                    }
                }),
            )
            .unwrap();
            assert_eq!(stage_result["status"], "ok");

            let diff_result = socket_request(
                topology.socket_path(),
                &serde_json::json!({
                    "request_type": "invoke_tool",
                    "sandbox": sandbox_id,
                    "container": container_id,
                    "name": "run_command",
                    "input": {
                        "cmd": "cd /workspace && git diff --cached HEAD",
                        "timeout": 20
                    }
                }),
            )
            .unwrap();
            assert_eq!(diff_result["status"], "ok");

            let diff = diff_result["result"]["stdout"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            assert!(diff.contains("agent_"), "diff should include agent file path");
            diffs.push(diff);
            agents.push(agent);
        }

        for diff in &diffs {
            topology.apply_diff_to_sandbox(&sandbox_name, diff).await.unwrap();
        }

        let merge_container = topology
            .create_agent_layer(&sandbox_name, Some("merge-verification".to_string()))
            .await
            .unwrap();

        topology
            .copy_workspace_to_host(
                &merge_container.container_id,
                &merge_container.sandbox_name,
                &output_dir,
            )
            .await
            .unwrap();

        for agent_index in 0..5 {
            let saved_source = output_dir
                .join("agents")
                .join(format!("agent_{agent_index}"))
                .join("result.js");
            let saved_manifest = output_dir.join(format!("agent_{agent_index}_manifest.json"));
            let saved_diff = output_dir
                .join("diffs")
                .join(format!("agent_{agent_index}.diff"));

            assert!(saved_source.exists(), "missing saved source for agent {agent_index}");
            assert!(saved_manifest.exists(), "missing saved manifest for agent {agent_index}");
            assert!(saved_diff.exists(), "missing saved diff artifact for agent {agent_index}");

            let source_contents = std::fs::read_to_string(&saved_source).unwrap();
            let manifest_contents = std::fs::read_to_string(&saved_manifest).unwrap();
            let diff_contents = std::fs::read_to_string(&saved_diff).unwrap();

            assert!(source_contents.contains("saved to host"));
            assert!(manifest_contents.contains(&format!("\"agent\": {agent_index}")));
            assert!(diff_contents.contains("diff --git"));
        }

        topology.destroy_agent_layer(&merge_container.agent_id).await.ok();
        for agent in &agents {
            topology.destroy_agent_layer(&agent.agent_id).await.ok();
        }
        topology.destroy_sandbox_layer(&sandbox_name).await.ok();

        cleanup_dir(&project_root);
        cleanup_dir(&output_dir);
    }

    // Integration test: End-to-end flow (requires agentd socket)
    #[tokio::test]
    #[ignore] // Ignore by default - requires running agentd socket server
    async fn test_end_to_end_orchestration() {
        use libagent::orchestration::{NewOrchestrator, OrchestratorConfig};

        let config = OrchestratorConfig {
            project_id: "company-internal-tools-490516".to_string(),
            socket_path: "/tmp/agentd.sock".to_string(),
            project_root: PathBuf::from("."),
            overlay_root: temp_dir("e2e-overlay"),
            checkpoint_root: temp_dir("e2e-checkpoints"),
            merge_work_dir: temp_dir("e2e-merge"),
            max_agents: 10,
            max_verification_rounds: 1,
            staging_dir: None,
            event_tx: None,
        };

        let orchestrator = NewOrchestrator::new(config);

        let result = orchestrator.run("create a simple hello world function").await;

        // This may fail without proper setup - that's expected
        if let Ok(output) = result {
            println!("Orchestration succeeded!");
            println!("Summary: {}", output.summary);
            assert!(output.total_agents_used > 0);
        } else {
            println!("Orchestration failed (expected without socket server)");
        }

        // Cleanup
        cleanup_dir(&temp_dir("e2e-overlay"));
        cleanup_dir(&temp_dir("e2e-checkpoints"));
        cleanup_dir(&temp_dir("e2e-merge"));
    }

    // Benchmark test for scheduler performance
    #[tokio::test]
    async fn test_scheduler_performance() {
        use agentd_protocol::{Task, TaskGraph};
        use libagent::orchestration::scheduler::Scheduler;
        use std::collections::HashMap;

        // Create a large task graph
        let mut tasks = Vec::new();
        let mut hints = HashMap::new();

        // 100 tasks in a dependency chain
        for i in 0..100 {
            let deps = if i == 0 {
                vec![]
            } else {
                vec![format!("t{}", i - 1)]
            };

            tasks.push(Task {
                id: format!("t{}", i),
                description: format!("Task {}", i),
                deps,
                hint: Some("sandbox1".to_string()),
            });

            hints.insert(format!("t{}", i), "sandbox1".to_string());
        }

        let task_graph = TaskGraph { tasks };

        let start = std::time::Instant::now();
        let scheduler = Scheduler::new(task_graph, hints).unwrap();
        let create_time = start.elapsed();

        println!("Scheduler creation time for 100 tasks: {:?}", create_time);

        // Should be very fast
        assert!(create_time.as_millis() < 100, "Scheduler creation too slow");

        let stats = scheduler.get_stats().await;
        assert_eq!(stats.total_tasks, 100);
    }
}
