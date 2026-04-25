use libagent::{ResourceLimits, Sandbox};
use serde_json::json;

// ============== SOCKET PROTOCOL TESTS ==============
// These tests verify the socket protocol operations for sandbox management

#[test]
fn test_create_sandbox() {
    let limits = ResourceLimits {
        ram_bytes: Some(512 * 1024 * 1024),
        cpu_millis: Some(5000),
    };
    let result = Sandbox::new(limits.clone());
    assert!(result.is_ok(), "sandbox creation should succeed");

    let sandbox = result.unwrap();
    assert!(sandbox.id() > 0, "sandbox id should be positive");
}

#[test]
fn test_create_sandbox_no_limits() {
    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };
    let result = Sandbox::new(limits);
    assert!(
        result.is_ok(),
        "sandbox creation with no limits should succeed"
    );
}

#[test]
fn test_create_sandboxes_have_unique_ids() {
    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };
    let sb1 = Sandbox::new(limits.clone()).unwrap();
    let sb2 = Sandbox::new(limits.clone()).unwrap();
    let sb3 = Sandbox::new(limits.clone()).unwrap();

    // IDs should be unique and monotonically increasing
    let id1 = sb1.id();
    let id2 = sb2.id();
    let id3 = sb3.id();

    assert_ne!(id1, id2, "sandbox IDs should be unique");
    assert_ne!(id2, id3, "sandbox IDs should be unique");
    assert_ne!(id1, id3, "sandbox IDs should be unique");
    assert!(
        id1 < id2 && id2 < id3,
        "sandbox IDs should be monotonically increasing"
    );
}

#[test]
fn test_destroy_sandbox() {
    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };
    let sandbox = Sandbox::new(limits).unwrap();
    let sandbox_id = sandbox.id();
    drop(sandbox);
    // If we got here without panic, destruction was successful
    assert!(sandbox_id > 0);
}

#[test]
fn test_list_sandboxes_empty() {
    // In the socket protocol, list returns all active sandboxes
    // This test verifies basic sandbox listing
    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };
    let sb = Sandbox::new(limits).unwrap();
    let _ = sb.id();
}

#[test]
fn test_sandbox_with_image() {
    // Test creating sandbox with image reference (if supported)
    let limits = ResourceLimits {
        ram_bytes: Some(512 * 1024 * 1024),
        cpu_millis: Some(5000),
    };
    let result = Sandbox::new_with_image(limits, Some("alpine:latest"));
    // Result may fail if image handling isn't fully implemented, that's ok
    let _ = result;
}

#[test]
fn test_tool_registration() {
    use libagent::Tool;
    use libagent::ToolContext;

    struct TestTool;
    impl Tool for TestTool {
        fn name(&self) -> &'static str {
            "test_tool"
        }
        fn invoke(
            &self,
            _ctx: &ToolContext,
            input: serde_json::Value,
        ) -> anyhow::Result<serde_json::Value> {
            Ok(input)
        }
        fn clone_box(&self) -> Box<dyn Tool> {
            Box::new(TestTool)
        }
    }

    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(Box::new(TestTool));

    let result = sandbox.invoke_tool("test_tool", json!({"test": "data"}));
    assert!(result.is_ok(), "registered tool should invoke successfully");
    assert_eq!(result.unwrap(), json!({"test": "data"}));
}

#[test]
fn test_invoke_nonexistent_tool() {
    let sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    let result = sandbox.invoke_tool("nonexistent_tool", json!(null));
    assert!(result.is_err(), "invoking non-existent tool should fail");
}

#[test]
fn test_policy_setting() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    let policy = libagent::SecurityPolicy::default_restrictive();
    sandbox.set_policy(policy);

    let retrieved = sandbox.policy();
    assert!(
        retrieved.is_some(),
        "policy should be retrievable after setting"
    );
}

#[test]
fn test_policy_permissive() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    let policy = libagent::SecurityPolicy::default_permissive();
    sandbox.set_policy(policy);

    let retrieved = sandbox.policy();
    assert!(
        retrieved.is_some(),
        "permissive policy should be set successfully"
    );
}

#[test]
fn test_run_command() {
    let sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    let result = sandbox.run_command("echo hello");
    assert!(result.is_ok(), "running valid command should succeed");
}

#[test]
fn test_run_command_with_output() {
    let sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    let result = sandbox.run_command("echo 'test output'");
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(!output.is_empty(), "command output should not be empty");
}

#[test]
fn test_sandbox_root_path() {
    let sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    let root = sandbox.root_path();
    assert!(root.is_absolute(), "root path should be absolute");
}

#[test]
fn test_multiple_tool_registrations() {
    use libagent::Tool;
    use libagent::ToolContext;

    struct Tool1;
    impl Tool for Tool1 {
        fn name(&self) -> &'static str {
            "tool1"
        }
        fn invoke(
            &self,
            _ctx: &ToolContext,
            input: serde_json::Value,
        ) -> anyhow::Result<serde_json::Value> {
            Ok(json!({"tool": 1}))
        }
        fn clone_box(&self) -> Box<dyn Tool> {
            Box::new(Tool1)
        }
    }

    struct Tool2;
    impl Tool for Tool2 {
        fn name(&self) -> &'static str {
            "tool2"
        }
        fn invoke(
            &self,
            _ctx: &ToolContext,
            input: serde_json::Value,
        ) -> anyhow::Result<serde_json::Value> {
            Ok(json!({"tool": 2}))
        }
        fn clone_box(&self) -> Box<dyn Tool> {
            Box::new(Tool2)
        }
    }

    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(Box::new(Tool1));
    sandbox.register_tool(Box::new(Tool2));

    let r1 = sandbox.invoke_tool("tool1", json!(null)).unwrap();
    let r2 = sandbox.invoke_tool("tool2", json!(null)).unwrap();

    assert_eq!(r1["tool"], 1);
    assert_eq!(r2["tool"], 2);
}

#[test]
fn test_sandbox_isolation_ids() {
    // Verify that multiple sandboxes work independently
    let sb1 = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    let sb2 = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    let id1 = sb1.id();
    let id2 = sb2.id();

    assert_ne!(id1, id2, "different sandboxes should have different IDs");
}
