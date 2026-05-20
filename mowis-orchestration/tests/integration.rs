use mowis_orchestration::events::{Event, EventBus};
use mowis_orchestration::plan::*;
use mowis_orchestration::tools::ToolAllowlist;
use mowis_orchestration::config::OrchConfig;

#[test]
fn test_plan_toml_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let plans_dir = tmp.path().join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();

    let plan_id = PlanId("20260520T143155Z-a4f8c1".into());
    let mut plan = Plan::new_draft(plan_id.clone(), "test goal", "conv-1");
    plan.plans_dir = plans_dir.clone();

    plan.task_graph.tasks.push(TaskNode {
        id: TaskId("t1".into()),
        title: "Add /healthz".into(),
        description: "Add health check endpoint".into(),
        deps: vec![],
        model_tier: ModelTier::Fast,
        tool_budget: 40,
        files_hint: vec!["src/main.rs".into()],
    });

    plan.task_graph.tasks.push(TaskNode {
        id: TaskId("t2".into()),
        title: "Test /healthz".into(),
        description: "Add test".into(),
        deps: vec![TaskId("t1".into())],
        model_tier: ModelTier::Fast,
        tool_budget: 20,
        files_hint: vec!["tests/main.rs".into()],
    });

    plan.save().unwrap();

    let loaded = Plan::load(&plans_dir, &plan_id).unwrap();
    assert_eq!(loaded.plan_id.0, "20260520T143155Z-a4f8c1");
    assert_eq!(loaded.task_graph.tasks.len(), 2);
    assert_eq!(loaded.task_graph.tasks[0].id.0, "t1");
    assert_eq!(loaded.task_graph.tasks[1].deps[0].0, "t1");
}

#[test]
fn test_plan_dag_cycle_detection() {
    let plan_id = PlanId("test-cycle".into());
    let mut plan = Plan::new_draft(plan_id, "test", "conv-1");

    plan.task_graph.tasks.push(TaskNode {
        id: TaskId("t1".into()),
        title: "Task 1".into(),
        description: "".into(),
        deps: vec![TaskId("t2".into())],
        model_tier: ModelTier::Fast,
        tool_budget: 10,
        files_hint: vec![],
    });
    plan.task_graph.tasks.push(TaskNode {
        id: TaskId("t2".into()),
        title: "Task 2".into(),
        description: "".into(),
        deps: vec![TaskId("t1".into())],
        model_tier: ModelTier::Fast,
        tool_budget: 10,
        files_hint: vec![],
    });

    assert!(plan.validate().is_err());
}

#[test]
fn test_plan_dag_valid() {
    let plan_id = PlanId("test-valid".into());
    let mut plan = Plan::new_draft(plan_id, "test", "conv-1");

    plan.task_graph.tasks.push(TaskNode {
        id: TaskId("t1".into()),
        title: "Task 1".into(),
        description: "".into(),
        deps: vec![],
        model_tier: ModelTier::Fast,
        tool_budget: 10,
        files_hint: vec![],
    });
    plan.task_graph.tasks.push(TaskNode {
        id: TaskId("t2".into()),
        title: "Task 2".into(),
        description: "".into(),
        deps: vec![TaskId("t1".into())],
        model_tier: ModelTier::Fast,
        tool_budget: 10,
        files_hint: vec![],
    });

    assert!(plan.validate().is_ok());
}

#[test]
fn test_tool_allowlist_allows_default() {
    let list = ToolAllowlist {
        allowed: vec![],
        denied_prefixes: vec![],
    };
    assert!(list.allows("read_file"));
    assert!(list.allows("anything"));
}

#[test]
fn test_tool_allowlist_restrictive() {
    let list = ToolAllowlist {
        allowed: vec!["read_file".into(), "list_files".into()],
        denied_prefixes: vec![],
    };
    assert!(list.allows("read_file"));
    assert!(list.allows("list_files"));
    assert!(!list.allows("write_file"));
}

#[test]
fn test_tool_allowlist_denied_prefix() {
    let list = ToolAllowlist {
        allowed: vec![],
        denied_prefixes: vec!["rm".into()],
    };
    assert!(list.allows("read_file"));
    assert!(!list.allows("rm -rf"));
}

#[test]
fn test_event_bus_delivers_to_subscribers() {
    let bus = EventBus::new();
    let mut rx1 = bus.subscribe();
    let mut rx2 = bus.subscribe();

    let plan_id = PlanId("test".into());
    bus.emit(Event::PlanDrafted {
        plan_id: plan_id.clone(),
        version: 1,
    });

    let ev1 = rx1.try_recv().unwrap();
    let ev2 = rx2.try_recv().unwrap();

    match ev1 {
        Event::PlanDrafted { plan_id: p, version } => {
            assert_eq!(p.0, "test");
            assert_eq!(version, 1);
        }
        _ => panic!("wrong event"),
    }
    match ev2 {
        Event::PlanDrafted { .. } => {}
        _ => panic!("wrong event"),
    }
}

#[test]
fn test_event_bus_lagged_subscriber() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    // Overflow the buffer
    for i in 0..2048 {
        bus.emit(Event::PlanDrafted {
            plan_id: PlanId(format!("plan-{}", i)),
            version: 1,
        });
    }

    // The subscriber should get Lagged error
    let mut got_lagged = false;
    loop {
        match rx.try_recv() {
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {
                got_lagged = true;
                break;
            }
            Err(_) => break,
            Ok(_) => continue,
        }
    }
    assert!(got_lagged);
}

#[test]
fn test_orch_config_default() {
    let cfg = OrchConfig::default();
    assert!(cfg.tiers.contains_key(&Tier::Conductor));
    assert!(cfg.tiers.contains_key(&Tier::Crew));
    assert_eq!(cfg.plans_dir.to_string_lossy(), ".mowis/plans");
}

#[test]
fn test_plan_history_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let plans_dir = tmp.path().join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();

    let plan_id = PlanId("test-history".into());
    let mut plan = Plan::new_draft(plan_id.clone(), "test goal", "conv-1");
    plan.plans_dir = plans_dir.clone();
    plan.overview = "version 1 overview".into();
    plan.save().unwrap();

    plan.snapshot_to_history().unwrap();
    plan.current_version = 2;
    plan.overview = "version 2 overview".into();
    plan.save().unwrap();

    let history_file = plans_dir
        .join("test-history")
        .join("history")
        .join("v1")
        .join("overview.md");
    assert!(history_file.exists());
    assert_eq!(std::fs::read_to_string(history_file).unwrap(), "version 1 overview");
}
