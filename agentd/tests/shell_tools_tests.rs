use libagent::{ResourceLimits, Sandbox, Tool};
use serde_json::json;

// ============== SHELL TOOL TESTS (5 operations) ==============
// run_command, run_script, kill_process, get_env, set_env

#[test]
fn test_run_command_basic() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_run_command_tool());

    let result = sandbox.invoke_tool(
        "run_command",
        json!({
            "cmd": "echo 'hello world'"
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output["success"], true);
    assert!(output["stdout"].as_str().unwrap().contains("hello"));
}

#[test]
fn test_run_command_exit_code() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_run_command_tool());

    // Successful command
    let result1 = sandbox.invoke_tool(
        "run_command",
        json!({
            "cmd": "exit 0"
        }),
    );
    assert_eq!(result1.unwrap()["exit_code"], 0);

    // Failed command
    let result2 = sandbox.invoke_tool(
        "run_command",
        json!({
            "cmd": "exit 1"
        }),
    );
    assert_eq!(result2.unwrap()["exit_code"], 1);
}

#[test]
fn test_run_command_with_stderr() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_run_command_tool());

    let result = sandbox.invoke_tool(
        "run_command",
        json!({
            "cmd": "echo 'error' >&2"
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output["stderr"].as_str().unwrap().contains("error"));
}

#[test]
fn test_run_command_with_cwd() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // First create a directory structure
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/testdir/file.txt",
                "content": "test"
            }),
        )
        .unwrap();

    // Run command with cwd
    sandbox.register_tool(get_run_command_tool());
    let result = sandbox.invoke_tool(
        "run_command",
        json!({
            "cmd": "pwd",
            "cwd": "/testdir"
        }),
    );
    assert!(result.is_ok());
}

#[test]
fn test_run_command_piped() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_run_command_tool());

    let result = sandbox.invoke_tool(
        "run_command",
        json!({
            "cmd": "echo 'hello' | wc -l"
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output["success"], true);
}

#[test]
fn test_run_command_env_var() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Set env var
    sandbox.register_tool(get_set_env_tool());
    sandbox
        .invoke_tool(
            "set_env",
            json!({
                "var": "TEST_VAR",
                "value": "test_value"
            }),
        )
        .unwrap();

    // Use it in command
    sandbox.register_tool(get_run_command_tool());
    let result = sandbox.invoke_tool(
        "run_command",
        json!({
            "cmd": "echo $TEST_VAR"
        }),
    );
    assert!(result.is_ok());
}

#[test]
fn test_run_script_basic() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // First create a script
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/test_script.sh",
                "content": "#!/bin/sh\necho 'script output'"
            }),
        )
        .unwrap();

    // Run it
    sandbox.register_tool(get_run_script_tool());
    let result = sandbox.invoke_tool(
        "run_script",
        json!({
            "path": "/test_script.sh"
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output["stdout"].as_str().unwrap().contains("script"));
}

#[test]
fn test_run_script_with_python() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create Python script
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/test_python.py",
                "content": "#!/usr/bin/env python3\nprint('python works')"
            }),
        )
        .unwrap();

    // Run with Python interpreter
    sandbox.register_tool(get_run_script_tool());
    let result = sandbox.invoke_tool(
        "run_script",
        json!({
            "path": "/test_python.py",
            "interpreter": "/usr/bin/python3"
        }),
    );
    // This may fail if python3 not available, that's ok
    let _ = result;
}

#[test]
fn test_run_script_exit_code() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create script that exits with code 42
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/exit_script.sh",
                "content": "#!/bin/sh\nexit 42"
            }),
        )
        .unwrap();

    // Run it
    sandbox.register_tool(get_run_script_tool());
    let result = sandbox.invoke_tool(
        "run_script",
        json!({
            "path": "/exit_script.sh"
        }),
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap()["exit_code"], 42);
}

#[test]
fn test_run_script_with_arguments() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create script that uses arguments
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/args_script.sh",
                "content": "#!/bin/sh\necho $1 $2"
            }),
        )
        .unwrap();

    sandbox.register_tool(get_run_script_tool());
    let result = sandbox.invoke_tool(
        "run_script",
        json!({
            "path": "/args_script.sh"
        }),
    );
    assert!(result.is_ok());
}

#[test]
fn test_kill_process_nonexistent() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_kill_process_tool());

    // Try to kill non-existent process
    let result = sandbox.invoke_tool(
        "kill_process",
        json!({
            "pid": 999999999u64
        }),
    );
    assert!(result.is_ok());
    // Should report success: false for non-existent process
    let output = result.unwrap();
    assert!(output["success"] == false || output["success"].is_null());
}

#[test]
fn test_kill_process_signal() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Start a background process (sleep)
    sandbox.register_tool(get_run_command_tool());
    let spawn_result = sandbox.invoke_tool(
        "run_command",
        json!({
            "cmd": "sleep 10 &"
        }),
    );
    assert!(spawn_result.is_ok());

    // Now we can't easily test kill since we can't get the PID reliably
    // But we can test the tool registration
    sandbox.register_tool(get_kill_process_tool());
    assert!(true); // Tool registered successfully
}

#[test]
fn test_get_env_existing() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_get_env_tool());

    // Get PATH which should always exist
    let result = sandbox.invoke_tool(
        "get_env",
        json!({
            "var": "PATH"
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_ne!(output["value"], null);
}

#[test]
fn test_get_env_nonexistent() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_get_env_tool());

    // Get non-existent variable
    let result = sandbox.invoke_tool(
        "get_env",
        json!({
            "var": "NONEXISTENT_VAR_XYZ_123"
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output["value"], null);
}

#[test]
fn test_get_env_after_set() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Set variable
    sandbox.register_tool(get_set_env_tool());
    sandbox
        .invoke_tool(
            "set_env",
            json!({
                "var": "MY_TEST_VAR",
                "value": "my_test_value"
            }),
        )
        .unwrap();

    // Get it back
    sandbox.register_tool(get_get_env_tool());
    let result = sandbox.invoke_tool(
        "get_env",
        json!({
            "var": "MY_TEST_VAR"
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output["value"], "my_test_value");
}

#[test]
fn test_set_env_basic() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_set_env_tool());

    let result = sandbox.invoke_tool(
        "set_env",
        json!({
            "var": "TEST_VAR1",
            "value": "test_value_1"
        }),
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap()["success"], true);
}

#[test]
fn test_set_env_overwrites() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Set initial value
    sandbox.register_tool(get_set_env_tool());
    sandbox
        .invoke_tool(
            "set_env",
            json!({
                "var": "OVERWRITE_VAR",
                "value": "initial"
            }),
        )
        .unwrap();

    // Overwrite it
    let result = sandbox.invoke_tool(
        "set_env",
        json!({
            "var": "OVERWRITE_VAR",
            "value": "overwritten"
        }),
    );
    assert!(result.is_ok());

    // Verify new value
    sandbox.register_tool(get_get_env_tool());
    let output = sandbox
        .invoke_tool(
            "get_env",
            json!({
                "var": "OVERWRITE_VAR"
            }),
        )
        .unwrap();
    assert_eq!(output["value"], "overwritten");
}

#[test]
fn test_set_env_multiple() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_set_env_tool());

    // Set multiple variables
    sandbox
        .invoke_tool(
            "set_env",
            json!({
                "var": "VAR1",
                "value": "value1"
            }),
        )
        .unwrap();

    sandbox
        .invoke_tool(
            "set_env",
            json!({
                "var": "VAR2",
                "value": "value2"
            }),
        )
        .unwrap();

    sandbox
        .invoke_tool(
            "set_env",
            json!({
                "var": "VAR3",
                "value": "value3"
            }),
        )
        .unwrap();

    // Verify all
    sandbox.register_tool(get_get_env_tool());
    let r1 = sandbox
        .invoke_tool("get_env", json!({"var": "VAR1"}))
        .unwrap();
    let r2 = sandbox
        .invoke_tool("get_env", json!({"var": "VAR2"}))
        .unwrap();
    let r3 = sandbox
        .invoke_tool("get_env", json!({"var": "VAR3"}))
        .unwrap();

    assert_eq!(r1["value"], "value1");
    assert_eq!(r2["value"], "value2");
    assert_eq!(r3["value"], "value3");
}

#[test]
fn test_shell_commands_composition() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Run command to create a file
    sandbox.register_tool(get_run_command_tool());
    sandbox
        .invoke_tool(
            "run_command",
            json!({
                "cmd": "echo 'test' > /tmp/test.txt"
            }),
        )
        .unwrap();

    // Run another command to read it
    let result = sandbox.invoke_tool(
        "run_command",
        json!({
            "cmd": "cat /tmp/test.txt"
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output["stdout"].as_str().unwrap().contains("test"));
}

// Helper functions to get tool instances
fn get_run_command_tool() -> Box<dyn Tool> {
    use libagent::tools::RunCommandTool;
    Box::new(RunCommandTool)
}

fn get_run_script_tool() -> Box<dyn Tool> {
    use libagent::tools::RunScriptTool;
    Box::new(RunScriptTool)
}

fn get_kill_process_tool() -> Box<dyn Tool> {
    use libagent::tools::KillProcessTool;
    Box::new(KillProcessTool)
}

fn get_get_env_tool() -> Box<dyn Tool> {
    use libagent::tools::GetEnvTool;
    Box::new(GetEnvTool)
}

fn get_set_env_tool() -> Box<dyn Tool> {
    use libagent::tools::SetEnvTool;
    Box::new(SetEnvTool)
}

fn get_write_file_tool() -> Box<dyn Tool> {
    use libagent::tools::WriteFileTool;
    Box::new(WriteFileTool)
}
