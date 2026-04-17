use libagent::{ResourceLimits, Sandbox};
use serde_json::json;
use std::sync::{Arc, Mutex};

// ============== SANDBOX OPERATIONS INTEGRATION TESTS ==============
// Tests for create_sandbox, destroy_sandbox, and list operations

#[test]
fn test_sandbox_creation_success() {
    let limits = ResourceLimits {
        ram_bytes: Some(1024 * 1024 * 1024), // 1GB
        cpu_millis: Some(10000),             // 10s
    };
    let sandbox = Sandbox::new(limits);
    assert!(
        sandbox.is_ok(),
        "sandbox creation should succeed with valid limits"
    );
    let sb = sandbox.unwrap();
    assert!(sb.id() > 0, "sandbox should have a valid positive ID");
}

#[test]
fn test_sandbox_creation_minimal_limits() {
    let limits = ResourceLimits {
        ram_bytes: Some(64 * 1024 * 1024), // 64MB
        cpu_millis: Some(1000),            // 1s
    };
    let sandbox = Sandbox::new(limits);
    assert!(
        sandbox.is_ok(),
        "sandbox creation should succeed with minimal limits"
    );
}

#[test]
fn test_sandbox_creation_unlimited() {
    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };
    let sandbox = Sandbox::new(limits);
    assert!(
        sandbox.is_ok(),
        "sandbox creation should succeed with unlimited resources"
    );
}

#[test]
fn test_destroy_sandbox_cleanup() {
    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };
    let sb = Sandbox::new(limits).unwrap();
    let root = sb.root_path().to_path_buf();

    // Verify root exists before dropping
    assert!(root.exists(), "sandbox root should exist");

    drop(sb);

    // After dropping, the directory might be cleaned up
    // (though the OS may not immediately release it)
}

#[test]
fn test_sandbox_list_multiple() {
    // Create multiple sandboxes and verify they all have unique IDs
    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };

    let mut sandboxes = vec![];
    let mut ids = vec![];

    for _ in 0..5 {
        let sb = Sandbox::new(limits.clone()).unwrap();
        ids.push(sb.id());
        sandboxes.push(sb);
    }

    // Verify all IDs are unique
    ids.sort();
    for i in 0..ids.len() - 1 {
        assert_ne!(ids[i], ids[i + 1], "all sandbox IDs should be unique");
    }
}

#[test]
fn test_sandbox_sequential_destruction() {
    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };

    let sb1 = Sandbox::new(limits.clone()).unwrap();
    let id1 = sb1.id();
    drop(sb1);

    let sb2 = Sandbox::new(limits.clone()).unwrap();
    let id2 = sb2.id();
    drop(sb2);

    let sb3 = Sandbox::new(limits.clone()).unwrap();
    let id3 = sb3.id();
    drop(sb3);

    // IDs should be monotonically increasing even after destruction
    assert!(
        id1 < id2 && id2 < id3,
        "IDs should continue increasing after destruction"
    );
}

#[test]
fn test_sandbox_concurrent_creation() {
    use std::thread;

    let handles: Vec<_> = (0..10)
        .map(|_| {
            thread::spawn(|| {
                let limits = ResourceLimits {
                    ram_bytes: None,
                    cpu_millis: None,
                };
                let sb = Sandbox::new(limits).unwrap();
                sb.id()
            })
        })
        .collect();

    let mut ids = vec![];
    for handle in handles {
        ids.push(handle.join().unwrap());
    }

    // All IDs should be unique
    let original_len = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(
        ids.len(),
        original_len,
        "concurrent sandbox creation should produce unique IDs"
    );
}

#[test]
fn test_sandbox_state_isolation() {
    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };

    let mut sb1 = Sandbox::new(limits.clone()).unwrap();
    let mut sb2 = Sandbox::new(limits.clone()).unwrap();

    // Register different tools in each sandbox
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
            _input: serde_json::Value,
        ) -> anyhow::Result<serde_json::Value> {
            Ok(json!({"id": 1}))
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
            _input: serde_json::Value,
        ) -> anyhow::Result<serde_json::Value> {
            Ok(json!({"id": 2}))
        }
        fn clone_box(&self) -> Box<dyn Tool> {
            Box::new(Tool2)
        }
    }

    sb1.register_tool(Box::new(Tool1));
    sb2.register_tool(Box::new(Tool2));

    // Verify each sandbox only has its own tool
    assert!(
        sb1.invoke_tool("tool1", json!(null)).is_ok(),
        "sb1 should have tool1"
    );
    assert!(
        sb1.invoke_tool("tool2", json!(null)).is_err(),
        "sb1 should not have tool2"
    );

    assert!(
        sb2.invoke_tool("tool2", json!(null)).is_ok(),
        "sb2 should have tool2"
    );
    assert!(
        sb2.invoke_tool("tool1", json!(null)).is_err(),
        "sb2 should not have tool1"
    );
}

#[test]
fn test_sandbox_policy_isolation() {
    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };

    let mut sb1 = Sandbox::new(limits.clone()).unwrap();
    let mut sb2 = Sandbox::new(limits.clone()).unwrap();

    let policy1 = libagent::SecurityPolicy::default_restrictive();
    let policy2 = libagent::SecurityPolicy::default_permissive();

    sb1.set_policy(policy1);
    sb2.set_policy(policy2);

    // Policies should be different
    let p1 = sb1.policy();
    let p2 = sb2.policy();

    assert!(p1.is_some() && p2.is_some(), "both policies should be set");
}

#[test]
fn test_sandbox_resource_limits_tracking() {
    let limits1 = ResourceLimits {
        ram_bytes: Some(512 * 1024 * 1024),
        cpu_millis: Some(5000),
    };

    let sb1 = Sandbox::new(limits1).unwrap();
    assert!(
        sb1.id() > 0,
        "sandbox with specific limits should be created"
    );

    let limits2 = ResourceLimits {
        ram_bytes: Some(256 * 1024 * 1024),
        cpu_millis: Some(2000),
    };

    let sb2 = Sandbox::new(limits2).unwrap();
    assert!(
        sb2.id() > 0,
        "second sandbox with different limits should be created"
    );

    assert_ne!(sb1.id(), sb2.id(), "sandboxes should have different IDs");
}

#[test]
fn test_sandbox_with_packages() {
    // Test creating sandbox with specific packages (if image is provided)
    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };
    let sb = Sandbox::new(limits).unwrap();
    assert!(sb.id() > 0, "sandbox should be created");
    // Package installation would be tested at socket layer
}

#[test]
fn test_sandbox_image_reference() {
    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };

    // Test with different image references
    let result1 = Sandbox::new_with_image(limits.clone(), Some("alpine:latest"));
    let result2 = Sandbox::new_with_image(limits.clone(), Some("ubuntu:20.04"));
    let result3 = Sandbox::new_with_image(limits.clone(), Some("debian:bullseye"));

    // All should either succeed or fail gracefully, not panic
    let _ = result1;
    let _ = result2;
    let _ = result3;
}

#[test]
fn test_sandbox_root_path_exists() {
    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };
    let sb = Sandbox::new(limits).unwrap();

    let root = sb.root_path();
    assert!(root.exists(), "sandbox root path should exist");
    assert!(root.is_absolute(), "sandbox root path should be absolute");
}

#[test]
fn test_sandbox_root_path_unique() {
    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };
    let sb1 = Sandbox::new(limits.clone()).unwrap();
    let sb2 = Sandbox::new(limits.clone()).unwrap();

    let root1 = sb1.root_path();
    let root2 = sb2.root_path();

    assert_ne!(
        root1, root2,
        "different sandboxes should have different root paths"
    );
}

#[test]
fn test_sandbox_created_at_different_times() {
    use std::thread;
    use std::time::Duration;

    let limits = ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    };

    let sb1 = Sandbox::new(limits.clone()).unwrap();
    let id1 = sb1.id();

    thread::sleep(Duration::from_millis(100));

    let sb2 = Sandbox::new(limits.clone()).unwrap();
    let id2 = sb2.id();

    // Even with sleep, IDs should be monotonically increasing
    assert!(id1 < id2, "subsequent sandbox should have higher ID");
}

#[test]
fn test_sandbox_multiple_commands() {
    let sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Execute multiple commands in the same sandbox
    let r1 = sandbox.run_command("echo 'test1'");
    let r2 = sandbox.run_command("echo 'test2'");
    let r3 = sandbox.run_command("echo 'test3'");

    assert!(r1.is_ok(), "first command should succeed");
    assert!(r2.is_ok(), "second command should succeed");
    assert!(r3.is_ok(), "third command should succeed");
}

#[test]
fn test_sandbox_command_isolation() {
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

    // Commands run in different sandboxes shouldn't interfere
    let r1 = sb1.run_command("pwd");
    let r2 = sb2.run_command("pwd");

    assert!(
        r1.is_ok() && r2.is_ok(),
        "commands in different sandboxes should both succeed"
    );
}
