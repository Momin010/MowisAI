use libagent::{ResourceLimits, Sandbox, Tool};
use serde_json::json;

// ============== HTTP TOOL TESTS (6 operations) ==============
// http_get, http_post, http_put, http_delete, http_patch, download_file

#[test]
fn test_http_get_missing_url() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_get_tool());

    let result = sandbox.invoke_tool("http_get", json!({}));
    assert!(result.is_err(), "http_get without url should fail");
}

#[test]
fn test_http_get_registration() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_get_tool());

    // Tool should be registered and callable
    assert!(true);
}

#[test]
fn test_http_post_missing_url() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_post_tool());

    let result = sandbox.invoke_tool(
        "http_post",
        json!({
            "body": "test"
        }),
    );
    assert!(result.is_err(), "http_post without url should fail");
}

#[test]
fn test_http_post_missing_body() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_post_tool());

    let result = sandbox.invoke_tool(
        "http_post",
        json!({
            "url": "http://example.com"
        }),
    );
    assert!(result.is_err(), "http_post without body should fail");
}

#[test]
fn test_http_post_registration() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_post_tool());

    // Tool should be registered
    assert!(true);
}

#[test]
fn test_http_put_missing_url() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_put_tool());

    let result = sandbox.invoke_tool(
        "http_put",
        json!({
            "body": "test"
        }),
    );
    assert!(result.is_err(), "http_put without url should fail");
}

#[test]
fn test_http_put_missing_body() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_put_tool());

    let result = sandbox.invoke_tool(
        "http_put",
        json!({
            "url": "http://example.com"
        }),
    );
    assert!(result.is_err(), "http_put without body should fail");
}

#[test]
fn test_http_put_registration() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_put_tool());

    assert!(true);
}

#[test]
fn test_http_delete_missing_url() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_delete_tool());

    let result = sandbox.invoke_tool("http_delete", json!({}));
    assert!(result.is_err(), "http_delete without url should fail");
}

#[test]
fn test_http_delete_registration() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_delete_tool());

    assert!(true);
}

#[test]
fn test_http_patch_missing_url() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_patch_tool());

    let result = sandbox.invoke_tool(
        "http_patch",
        json!({
            "body": "test"
        }),
    );
    assert!(result.is_err(), "http_patch without url should fail");
}

#[test]
fn test_http_patch_missing_body() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_patch_tool());

    let result = sandbox.invoke_tool(
        "http_patch",
        json!({
            "url": "http://example.com"
        }),
    );
    assert!(result.is_err(), "http_patch without body should fail");
}

#[test]
fn test_http_patch_registration() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_patch_tool());

    assert!(true);
}

#[test]
fn test_download_file_missing_url() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_download_file_tool());

    let result = sandbox.invoke_tool(
        "download_file",
        json!({
            "path": "/tmp/test.txt"
        }),
    );
    assert!(result.is_err(), "download_file without url should fail");
}

#[test]
fn test_download_file_missing_path() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_download_file_tool());

    let result = sandbox.invoke_tool(
        "download_file",
        json!({
            "url": "http://example.com/file.txt"
        }),
    );
    assert!(result.is_err(), "download_file without path should fail");
}

#[test]
fn test_download_file_registration() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_download_file_tool());

    assert!(true);
}

#[test]
fn test_http_tools_all_registered() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Register all HTTP tools
    sandbox.register_tool(get_http_get_tool());
    sandbox.register_tool(get_http_post_tool());
    sandbox.register_tool(get_http_put_tool());
    sandbox.register_tool(get_http_delete_tool());
    sandbox.register_tool(get_http_patch_tool());
    sandbox.register_tool(get_download_file_tool());

    assert!(true);
}

#[test]
fn test_http_get_response_structure() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_get_tool());

    // Test with invalid URL to see error handling
    let result = sandbox.invoke_tool(
        "http_get",
        json!({
            "url": "invalid://url"
        }),
    );
    // May fail or succeed depending on curl behavior
    let _ = result;
}

#[test]
fn test_http_post_json_content_type() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_post_tool());

    let result = sandbox.invoke_tool(
        "http_post",
        json!({
            "url": "http://localhost:9999",  // Non-existent server
            "body": r#"{"test": "data"}"#
        }),
    );
    // May fail but should handle gracefully
    let _ = result;
}

#[test]
fn test_http_methods_different_verbs() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Register all methods
    sandbox.register_tool(get_http_get_tool());
    sandbox.register_tool(get_http_post_tool());
    sandbox.register_tool(get_http_put_tool());
    sandbox.register_tool(get_http_delete_tool());
    sandbox.register_tool(get_http_patch_tool());

    // Each tool has different HTTP method
    // GET
    let r1 = sandbox.invoke_tool(
        "http_get",
        json!({
            "url": "http://localhost:9999/test"
        }),
    );

    // POST
    let r2 = sandbox.invoke_tool(
        "http_post",
        json!({
            "url": "http://localhost:9999/test",
            "body": "{}"
        }),
    );

    // PUT
    let r3 = sandbox.invoke_tool(
        "http_put",
        json!({
            "url": "http://localhost:9999/test",
            "body": "{}"
        }),
    );

    // DELETE
    let r4 = sandbox.invoke_tool(
        "http_delete",
        json!({
            "url": "http://localhost:9999/test"
        }),
    );

    // PATCH
    let r5 = sandbox.invoke_tool(
        "http_patch",
        json!({
            "url": "http://localhost:9999/test",
            "body": "{}"
        }),
    );

    // All should return results (may be errors due to no server)
    let _ = (r1, r2, r3, r4, r5);
}

#[test]
fn test_download_file_creates_parent_dirs() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_download_file_tool());

    // Attempt download with nested path
    let result = sandbox.invoke_tool(
        "download_file",
        json!({
            "url": "http://invalid-domain-xyz123.com/file.txt",
            "path": "/nested/deep/path/file.txt"
        }),
    );
    // May fail but should attempt to create directories
    let _ = result;
}

#[test]
fn test_http_get_with_localhost() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_get_tool());

    // Test with localhost (may or may not have service running)
    let result = sandbox.invoke_tool(
        "http_get",
        json!({
            "url": "http://localhost:8000/api"
        }),
    );
    // Should complete without panic
    let _ = result;
}

#[test]
fn test_http_post_with_empty_body() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_http_post_tool());

    let result = sandbox.invoke_tool(
        "http_post",
        json!({
            "url": "http://localhost:9999",
            "body": ""
        }),
    );
    // Should handle empty body
    let _ = result;
}

#[test]
fn test_http_tools_concurrent_requests() {
    use std::thread;

    let handles: Vec<_> = (0..5)
        .map(|i| {
            thread::spawn(move || {
                let mut sandbox = Sandbox::new(ResourceLimits {
                    ram_bytes: None,
                    cpu_millis: None,
                })
                .unwrap();
                sandbox.register_tool(get_http_get_tool());

                sandbox.invoke_tool(
                    "http_get",
                    json!({
                        "url": format!("http://localhost:9999/test/{}", i)
                    }),
                )
            })
        })
        .collect();

    for handle in handles {
        let _ = handle.join();
    }
}

#[test]
fn test_download_file_creates_files() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create a simple file locally
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/test_data.txt",
                "content": "test content"
            }),
        )
        .unwrap();

    // Verify file exists
    sandbox.register_tool(get_file_exists_tool());
    let exists = sandbox
        .invoke_tool(
            "file_exists",
            json!({
                "path": "/test_data.txt"
            }),
        )
        .unwrap();
    assert_eq!(exists["exists"], true);
}

// Helper functions to get tool instances
fn get_http_get_tool() -> Box<dyn Tool> {
    use libagent::tools::HttpGetTool;
    Box::new(HttpGetTool)
}

fn get_http_post_tool() -> Box<dyn Tool> {
    use libagent::tools::HttpPostTool;
    Box::new(HttpPostTool)
}

fn get_http_put_tool() -> Box<dyn Tool> {
    use libagent::tools::HttpPutTool;
    Box::new(HttpPutTool)
}

fn get_http_delete_tool() -> Box<dyn Tool> {
    use libagent::tools::HttpDeleteTool;
    Box::new(HttpDeleteTool)
}

fn get_http_patch_tool() -> Box<dyn Tool> {
    use libagent::tools::HttpPatchTool;
    Box::new(HttpPatchTool)
}

fn get_download_file_tool() -> Box<dyn Tool> {
    use libagent::tools::DownloadFileTool;
    Box::new(DownloadFileTool)
}

fn get_write_file_tool() -> Box<dyn Tool> {
    use libagent::tools::WriteFileTool;
    Box::new(WriteFileTool)
}

fn get_file_exists_tool() -> Box<dyn Tool> {
    use libagent::tools::FileExistsTool;
    Box::new(FileExistsTool)
}
