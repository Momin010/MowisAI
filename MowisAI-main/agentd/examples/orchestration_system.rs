/// End-to-end example demonstrating the MowisAI orchestration system
///
/// This example shows how the components work together:
/// 1. Global Orchestrator receives a user task
/// 2. Analyzes and builds dependency graph
/// 3. Provisions sandboxes via Runtime
/// 4. Creates Local Hub Agents in each sandbox
/// 5. Work is distributed to Worker Agents
/// 6. Results are collected and aggregated

use libagent::orchestrator::{GlobalOrchestrator, OrchestratorConfig};
use libagent::runtime::Runtime;
use libagent::hub_agent::{LocalHubAgent, HubAgentConfig};
use libagent::worker_agent::{WorkerAgent, WorkerConfig};
use libagent::protocol::*;
use std::collections::HashMap;

/// Example: Complete multi-team task execution
fn main() {
    println!("\n=== MowisAI System Design - End-to-End Example ===\n");

    // Configure the Global Orchestrator
    let orchestrator_config = OrchestratorConfig {
        runtime_socket_base: "/tmp/mowisai-sockets".to_string(),
        max_total_sandboxes: 10,
        task_timeout_secs: 3600,
        health_check_interval_secs: 10,
        llm_analysis_enabled: false, // set to true to use Claude for analysis
    };

    let orchestrator = GlobalOrchestrator::new(orchestrator_config);

    // Example 1: Simple backend task
    println!("[Example 1] Building a REST API backend");
    example_backend_task(&orchestrator);

    // Example 2: Multi-team web application
    println!("\n[Example 2] Building a complete web application");
    example_full_stack_task(&orchestrator);

    // Example 3: Dependency-based task execution
    println!("\n[Example 3] Tasks with dependencies");
    example_dependent_tasks();

    println!("\n=== All examples completed successfully ===\n");
}

/// Example 1: Backend API task
fn example_backend_task(orchestrator: &libagent::orchestrator::GlobalOrchestrator) {
    let user_task = "Build a REST API with Express.js and PostgreSQL database with CRUD operations for users".to_string();
    
    match orchestrator.execute_task(user_task.clone()) {
        Ok(session_id) => {
            println!("  ✓ Task execution started: {}", session_id);
            
            if let Some(session) = orchestrator.get_session_status(&session_id) {
                println!("  ✓ Session created with {} tasks", session.completed_tasks.len());
                println!("  ✓ Provisioning spec: {} sandboxes", session.provisioning_spec.num_sandboxes);
            }
        }
        Err(e) => println!("  ✗ Error: {:?}", e),
    }
}

/// Example 2: Full-stack web application with frontend and testing
fn example_full_stack_task(orchestrator: &libagent::orchestrator::GlobalOrchestrator) {
    let orchestrator_config = OrchestratorConfig {
        runtime_socket_base: "/tmp/mowisai-sockets".to_string(),
        max_total_sandboxes: 10,
        task_timeout_secs: 3600,
        health_check_interval_secs: 10,
        llm_analysis_enabled: false,
    };

    let orch = GlobalOrchestrator::new(orchestrator_config);
    let user_task = "Build a full-stack web application with React frontend, Node.js API backend, and comprehensive unit and integration tests".to_string();
    
    match orch.execute_task(user_task) {
        Ok(session_id) => {
            println!("  ✓ Full-stack task execution started: {}", session_id);
            
            if let Some(session) = orch.get_session_status(&session_id) {
                println!("  ✓ Teams created: {} tasks", session.completed_tasks.len());
                println!("  ✓ Execution stages: {}", session.dependency_graph.execution_order.len());
                println!("  ✓ Estimated resources: {} containers", session.provisioning_spec.sandbox_specs.len() * 10);
            }
        }
        Err(e) => println!("  ✗ Error: {:?}", e),
    }
}

/// Example 3: Orchestration of Local Hub Agents with Worker Agents
fn example_dependent_tasks() {
    println!("  Setting up Local Hub Agent with worker pool...");

    // Create a Local Hub Agent
    let hub_config = HubAgentConfig {
        team_id: "team-backend".to_string(),
        sandbox_id: "sandbox-1".to_string(),
        max_workers: 10,
        socket_path: "/tmp/hub-backend.sock".to_string(),
        peer_sockets: HashMap::new(),
    };

    let hub = LocalHubAgent::new(hub_config);

    // Initialize worker pool
    let container_ids = vec!["c1".to_string(), "c2".to_string(), "c3".to_string()];
    hub.init_worker_pool(container_ids).expect("Failed to init worker pool");
    println!("  ✓ Worker pool initialized with {} workers", hub.list_workers().len());

    // Receive a team task
    let team_task = TeamTask {
        task_id: "task-backend-1".to_string(),
        team_id: "team-backend".to_string(),
        description: "Build a REST API with user authentication, database access, and error handling".to_string(),
        dependencies: vec![],
        estimated_complexity: 150,
        timeout_secs: 3600,
        context: serde_json::json!({"task_type": "backend"}),
    };

    hub.receive_team_task(team_task).expect("Failed to receive task");
    println!("  ✓ Team task received and ready for breakdown");

    // Break down task into worker assignments
    match hub.break_down_task() {
        Ok(assignments) => {
            println!("  ✓ Task broken down into {} worker assignments", assignments.len());
            
            for (i, assignment) in assignments.iter().enumerate() {
                println!("    - Task {}: {} (tools: {})", 
                    i + 1, 
                    assignment.worker_name, 
                    assignment.tools_available.join(", ")
                );

                // Create a worker for this assignment
                let worker_config = WorkerConfig {
                    worker_name: assignment.worker_name.clone(),
                    team_id: "team-backend".to_string(),
                    sandbox_id: "sandbox-1".to_string(),
                    container_id: format!("c{}", i + 1),
                    agentd_socket: "/tmp/agentd.sock".to_string(),
                    hub_agent_socket: "/tmp/hub-backend.sock".to_string(),
                    api_key: "test-api-key".to_string(),
                };

                let worker = WorkerAgent::new(worker_config);

                // Execute the assignment
                worker.receive_assignment(assignment.clone()).expect("Failed to assign");
                worker.execute_task().expect("Failed to execute task");

                // Get completion
                match worker.create_completion() {
                    Ok(completion) => {
                        println!("    ✓ {} completed successfully", completion.worker_name);
                        hub.record_worker_completion(completion).expect("Failed to record");
                    }
                    Err(e) => println!("    ✗ {} failed: {:?}", assignment.worker_name, e),
                }

                // Signal idle to prepare for next task
                let _ = worker.signal_idle();
            }
        }
        Err(e) => println!("  ✗ Task breakdown failed: {:?}", e),
    }

    // Create final completion report
    match hub.create_completion_report() {
        Ok(report) => {
            println!("  ✓ Team completion report: success={}, workers={}", 
                report.success, 
                report.output.get("metadata")
                    .and_then(|m| m.get("worker_count"))
                    .and_then(|c| c.as_u64())
                    .unwrap_or(0)
            );
        }
        Err(e) => println!("  ✗ Failed to create report: {:?}", e),
    }
}

/*
=== Architecture Overview ===

Global Orchestrator
├─ Task Analysis & Planning
│  ├─ Decompose task into team tasks
│  ├─ Build dependency graph
│  └─ Estimate resource allocation
├─ Sandbox Provisioning (via Runtime)
│  ├─ Create sandboxes with OS + packages
│  └─ Create container pools
├─ Team Task Assignment
│  └─ Send work to Local Hub Agents
└─ Result Collection
   └─ Aggregate outputs

Runtime (Infrastructure Manager)
├─ Sandbox Lifecycle
│  ├─ Create/destroy sandboxes
│  └─ Manage containers
├─ Resource Enforcement
│  ├─ RAM/CPU limits
│  └─ Health monitoring
└─ Container Control
   ├─ Pause/resume (idle management)
   └─ Incremental provisioning

Local Hub Agent (in each Sandbox)
├─ Receive team task
├─ Break down into worker assignments
├─ Manage worker pool
├─ Coordinate with peer Hub Agents (via sockets)
└─ Report completion and aggregated results

Worker Agent (in each Container)
├─ Execute assigned work
├─ Make LLM calls (planning, reasoning)
├─ Invoke tools via agentd
├─ Test work quality
└─ Report completion and idle state

agentd (Unchanged)
├─ Tool execution (shell, filesystem, git, etc.)
├─ Unix socket server
└─ Sandbox/container primitives

=== Communication Patterns ===

Synchronous:
- Global Orchestrator → Runtime (provisioning)
- Global Orchestrator → Local Hub Agent (task assignment)
- Local Hub Agent ↔ Local Hub Agent (via sockets)
- Worker Agent → Local Hub Agent (completion signals)

Asynchronous:
- Worker Agent → Runtime (idle signals)
- Runtime → Global Orchestrator (health status)

=== Task Execution Flow ===

1. User provides task
2. Global Orchestrator analyzes complexity
3. Orchestrator builds dependency DAG
4. Orchestrator provisions sandboxes
5. Orchestrator sends team tasks to Hub Agents
6. Hub Agents break down work
7. Hub Agents assign to workers
8. Workers execute (with LLM reasoning)
9. Workers test and report
10. Hub Agents aggregate results
11. Global Orchestrator collects final output

=== Key Design Principles ===

✓ Separation of Concerns
  - Orchestrator: planning & coordination
  - Runtime: infrastructure only
  - Hub Agents: team-level management
  - Workers: task execution
  
✓ Scalability
  - Horizontal: add more sandboxes
  - Vertical: increase container count
  - Dynamic provisioning supported
  
✓ Fault Isolation
  - Sandbox failures don't affect others
  - Container crashes isolated
  - Hub Agent death detected via timeout
  
✓ Clean Communication
  - Socket-based RPC between teams
  - Protocol buffers for serialization
  - Dependency tracking for sequencing
*/
