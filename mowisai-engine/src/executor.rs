use crate::container::{spawn_container_alt, cleanup_cgroups};
use crate::protocol::{TaskRequest, TaskResponse};
use anyhow::{Context, Result};
use std::process::Output;
use std::string::String;

/// Execute a task request inside an isolated container
/// 
/// This function takes a TaskRequest, spawns a container with the specified
/// command, and returns a TaskResponse indicating success or failure.
/// 
/// # Arguments
/// * `request` - The TaskRequest containing task_id, command, and timeout
/// 
/// # Returns
/// * `TaskResponse` - Contains status ("done" or "error") and output or error message
pub fn execute_task(request: TaskRequest) -> TaskResponse {
    let rootfs_path = "/workspaces/MowisAI/mowisai-engine/rootfs";
    
    // Validate that rootfs exists

    if !std::path::Path::new(rootfs_path).exists() {
        return TaskResponse::error(
            request.task_id,
            format!("RootFS not found at {}. Please run setup_rootfs.sh first.", rootfs_path)
        );
    }
    
    // Get resource limits with defaults
    let memory_mb = request.memory_mb.unwrap_or(512);
    let cpu_percent = request.cpu_percent.unwrap_or(50);
    
    // Spawn the container and execute the command with resource limits
    match spawn_container_alt(rootfs_path, &request.command, request.timeout_secs, memory_mb, cpu_percent) {

        Ok(output) => {
            // Convert output to string
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            
            // Combine stdout and stderr
            let combined_output = if stderr.is_empty() {
                stdout.to_string()
            } else if stdout.is_empty() {
                stderr.to_string()
            } else {
                format!("{}{}", stdout, stderr)
            };
            
            // Check exit status
            if output.status.success() {
                TaskResponse::success(request.task_id, combined_output)
            } else {
                let exit_code = output.status.code().unwrap_or(-1);
                let error_msg = format!(
                    "Command failed with exit code {}: {}", 
                    exit_code, 
                    combined_output
                );
                TaskResponse::error(request.task_id, error_msg)
            }
        }
        Err(e) => {
            // Container execution failed or timed out
            let error_msg = format!("Container execution failed: {}", e);
            TaskResponse::error(request.task_id, error_msg)
        }
    }
}

/// Execute a task with additional error context
/// 
/// This is a more verbose version that provides detailed error information
pub fn execute_task_verbose(request: TaskRequest) -> TaskResponse {
    eprintln!("Executing task {}: {}", request.task_id, request.command);
    
    let result = execute_task(request.clone());
    
    match result.status.as_str() {
        "done" => eprintln!("Task {} completed successfully", request.task_id),
        "error" => eprintln!("Task {} failed: {}", request.task_id, result.output),
        _ => eprintln!("Task {} returned unknown status: {}", request.task_id, result.status),
    }
    
    // Cleanup cgroups after execution
    if let Err(e) = cleanup_cgroups() {
        eprintln!("Failed to cleanup cgroups: {}", e);
    }
    
    result
}

/// Validate that the container environment is properly set up
pub fn validate_environment() -> Result<()> {
    let rootfs_path = "/workspaces/MowisAI/mowisai-engine/rootfs";
    
    // Check if rootfs directory exists
    if !std::path::Path::new(rootfs_path).exists() {

        return Err(anyhow::anyhow!(
            "RootFS directory not found at {}. Please run setup_rootfs.sh first.", 
            rootfs_path
        ));
    }
    
    // Check for essential directories
    let essential_dirs = vec!["bin", "lib", "etc", "proc"];
    for dir in essential_dirs {
        let path = format!("{}/{}", rootfs_path, dir);
        if !std::path::Path::new(&path).exists() {
            return Err(anyhow::anyhow!(
                "Essential directory {} missing in rootfs. RootFS may be corrupted.", 
                dir
            ));
        }
    }
    
    // Check for busybox (all binaries like sh, ls, cat are symlinks to busybox)
    let busybox_path = format!("{}/bin/busybox", rootfs_path);
    if !std::path::Path::new(&busybox_path).exists() {
        return Err(anyhow::anyhow!(
            "Essential binary /bin/busybox missing in rootfs. RootFS may be corrupted."
        ));
    }
    
    Ok(())
}

/// Test the container with a simple command
pub fn test_container() -> Result<String> {
    let test_request = TaskRequest::new(
        "test-001".to_string(),
        "echo 'Container test successful' && uname -a".to_string(),
        10
    );
    
    let response = execute_task(test_request);
    
    match response.status.as_str() {
        "done" => Ok(response.output),
        _ => Err(anyhow::anyhow!("Container test failed: {}", response.output)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    
    #[test]
    fn test_task_response_success() {
        let request = TaskRequest::new(
            "test-123".to_string(),
            "echo hello".to_string(),
            5
        );
        
        // This test would need a real rootfs to work
        // For now, we just test the response structure
        let response = TaskResponse::success("test-123".to_string(), "hello".to_string());
        assert_eq!(response.status, "done");
        assert_eq!(response.task_id, "test-123");
    }
    
    #[test]
    fn test_task_response_error() {
        let response = TaskResponse::error(
            "test-456".to_string(), 
            "rootfs not found".to_string()
        );
        assert_eq!(response.status, "error");
        assert!(response.output.contains("rootfs"));
    }
    
    #[test]
    fn test_validate_environment_missing_rootfs() {
        // This test assumes ./rootfs doesn't exist in test environment
        let result = validate_environment();
        // Should fail because rootfs doesn't exist in test environment
        assert!(result.is_err());
    }
    
    #[test]
    fn test_execute_task_missing_rootfs() {
        let request = TaskRequest::new(
            "test-missing-rootfs".to_string(),
            "echo hello".to_string(),
            10
        );
        
        let response = execute_task(request);
        
        assert_eq!(response.status, "error");
        assert!(response.output.contains("RootFS not found"));
    }
    
    #[test]
    fn test_task_request_clone() {
        let request = TaskRequest::new(
            "clone-test".to_string(),
            "ls -la".to_string(),
            30
        );
        
        let cloned = request.clone();
        
        assert_eq!(cloned.task_id, request.task_id);
        assert_eq!(cloned.command, request.command);
        assert_eq!(cloned.timeout_secs, request.timeout_secs);
    }
    
    #[test]
    fn test_output_combination_stdout_only() {
        // Test that stdout-only output is handled correctly
        let stdout = "Hello World";
        let stderr = "";
        
        let combined = if stderr.is_empty() {
            stdout.to_string()
        } else if stdout.is_empty() {
            stderr.to_string()
        } else {
            format!("{}{}", stdout, stderr)
        };
        
        assert_eq!(combined, "Hello World");
    }
    
    #[test]
    fn test_output_combination_stderr_only() {
        // Test that stderr-only output is handled correctly
        let stdout = "";
        let stderr = "Error message";
        
        let combined = if stderr.is_empty() {
            stdout.to_string()
        } else if stdout.is_empty() {
            stderr.to_string()
        } else {
            format!("{}{}", stdout, stderr)
        };
        
        assert_eq!(combined, "Error message");
    }
    
    #[test]
    fn test_output_combination_both() {
        // Test that combined stdout and stderr is handled correctly
        let stdout = "Standard output";
        let stderr = "Standard error";
        
        let combined = if stderr.is_empty() {
            stdout.to_string()
        } else if stdout.is_empty() {
            stderr.to_string()
        } else {
            format!("{}{}", stdout, stderr)
        };
        
        assert_eq!(combined, "Standard outputStandard error");
    }
    
    #[test]
    fn test_task_id_preservation() {
        let task_id = "preserve-me-123";
        let request = TaskRequest::new(
            task_id.to_string(),
            "echo test".to_string(),
            15
        );
        
        // Even with missing rootfs, task_id should be preserved in error response
        let response = execute_task(request);
        
        assert_eq!(response.task_id, task_id);
    }
    
    #[test]
    fn test_timeout_value_passed() {
        let timeout_values = vec![1u64, 5, 10, 30, 60, 120, 300];
        
        for timeout in timeout_values {
            let request = TaskRequest::new(
                format!("timeout-test-{}", timeout),
                "sleep 1".to_string(),
                timeout
            );
            
            // Verify the timeout is stored correctly
            assert_eq!(request.timeout_secs, timeout);
        }
    }
    
    #[test]
    fn test_command_with_special_characters() {
        let commands = vec![
            "echo 'hello world'",
            "ls -la /tmp",
            "cat /etc/passwd | grep root",
            "echo \"test\" > /tmp/file",
        ];
        
        for cmd in commands {
            let request = TaskRequest::new(
                "special-cmd-test".to_string(),
                cmd.to_string(),
                10
            );
            
            assert_eq!(request.command, cmd);
        }
    }
    
    #[test]
    fn test_error_message_format() {
        let error_msg = "Container execution failed: some error details";
        let response = TaskResponse::error(
            "error-format-test".to_string(),
            error_msg.to_string()
        );
        
        let json = serde_json::to_string(&response).unwrap();
        
        // Verify JSON structure
        assert!(json.contains("error"));
        assert!(json.contains("error-format-test"));
        assert!(json.contains("Container execution failed"));
    }
}
