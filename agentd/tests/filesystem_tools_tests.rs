use libagent::{ResourceLimits, Sandbox, Tool, ToolContext};
use serde_json::json;
use std::path::PathBuf;

// ============== FILESYSTEM TOOL TESTS (11 operations) ==============
// read_file, write_file, append_file, delete_file, copy_file, move_file,
// list_files, create_directory, delete_directory, get_file_info, file_exists

#[test]
fn test_read_file_basic() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // First write a file
    sandbox.register_tool(get_write_file_tool());
    let write_result = sandbox.invoke_tool(
        "write_file",
        json!({
            "path": "/test_read.txt",
            "content": "hello world"
        }),
    );
    assert!(write_result.is_ok());

    // Then read it
    sandbox.register_tool(get_read_file_tool());
    let read_result = sandbox.invoke_tool(
        "read_file",
        json!({
            "path": "/test_read.txt"
        }),
    );
    assert!(read_result.is_ok());
    let result = read_result.unwrap();
    assert_eq!(result["content"], "hello world");
}

#[test]
fn test_read_file_nonexistent() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_read_file_tool());

    let result = sandbox.invoke_tool(
        "read_file",
        json!({
            "path": "/nonexistent_file.txt"
        }),
    );
    assert!(result.is_err(), "reading non-existent file should fail");
}

#[test]
fn test_write_file_basic() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_write_file_tool());

    let result = sandbox.invoke_tool(
        "write_file",
        json!({
            "path": "/test_write.txt",
            "content": "test content"
        }),
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap()["success"], true);
}

#[test]
fn test_write_file_overwrites_existing() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_write_file_tool());

    // Write first content
    let r1 = sandbox.invoke_tool(
        "write_file",
        json!({
            "path": "/test_overwrite.txt",
            "content": "first content"
        }),
    );
    assert!(r1.is_ok());

    // Write second content (should overwrite)
    let r2 = sandbox.invoke_tool(
        "write_file",
        json!({
            "path": "/test_overwrite.txt",
            "content": "second content"
        }),
    );
    assert!(r2.is_ok());

    // Verify the second content is there
    sandbox.register_tool(get_read_file_tool());
    let read_result = sandbox.invoke_tool(
        "read_file",
        json!({
            "path": "/test_overwrite.txt"
        }),
    );
    assert_eq!(read_result.unwrap()["content"], "second content");
}

#[test]
fn test_write_file_creates_directories() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_write_file_tool());

    let result = sandbox.invoke_tool(
        "write_file",
        json!({
            "path": "/nested/dirs/test_file.txt",
            "content": "nested content"
        }),
    );
    assert!(
        result.is_ok(),
        "write_file should create parent directories"
    );
}

#[test]
fn test_append_file_basic() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Write initial content
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/test_append.txt",
                "content": "line 1\n"
            }),
        )
        .unwrap();

    // Append content
    sandbox.register_tool(get_append_file_tool());
    let result = sandbox.invoke_tool(
        "append_file",
        json!({
            "path": "/test_append.txt",
            "content": "line 2\n"
        }),
    );
    assert!(result.is_ok());

    // Verify both lines are there
    sandbox.register_tool(get_read_file_tool());
    let content = sandbox
        .invoke_tool(
            "read_file",
            json!({
                "path": "/test_append.txt"
            }),
        )
        .unwrap();
    assert_eq!(content["content"], "line 1\nline 2\n");
}

#[test]
fn test_append_file_creates_if_missing() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_append_file_tool());

    let result = sandbox.invoke_tool(
        "append_file",
        json!({
            "path": "/new_append_file.txt",
            "content": "new content"
        }),
    );
    assert!(
        result.is_ok(),
        "append_file should create file if it doesn't exist"
    );
}

#[test]
fn test_delete_file_basic() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create a file
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/test_delete.txt",
                "content": "to be deleted"
            }),
        )
        .unwrap();

    // Delete it
    sandbox.register_tool(get_delete_file_tool());
    let result = sandbox.invoke_tool(
        "delete_file",
        json!({
            "path": "/test_delete.txt"
        }),
    );
    assert!(result.is_ok());

    // Verify it's gone
    sandbox.register_tool(get_file_exists_tool());
    let exists = sandbox
        .invoke_tool(
            "file_exists",
            json!({
                "path": "/test_delete.txt"
            }),
        )
        .unwrap();
    assert_eq!(exists["exists"], false);
}

#[test]
fn test_delete_file_nonexistent() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_delete_file_tool());

    let result = sandbox.invoke_tool(
        "delete_file",
        json!({
            "path": "/nonexistent_delete.txt"
        }),
    );
    assert!(result.is_err(), "deleting non-existent file should fail");
}

#[test]
fn test_copy_file_basic() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create source file
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/source.txt",
                "content": "source content"
            }),
        )
        .unwrap();

    // Copy it
    sandbox.register_tool(get_copy_file_tool());
    let result = sandbox.invoke_tool(
        "copy_file",
        json!({
            "from": "/source.txt",
            "to": "/dest.txt"
        }),
    );
    assert!(result.is_ok());

    // Verify both files exist with same content
    sandbox.register_tool(get_read_file_tool());
    let source_content = sandbox
        .invoke_tool(
            "read_file",
            json!({
                "path": "/source.txt"
            }),
        )
        .unwrap();
    let dest_content = sandbox
        .invoke_tool(
            "read_file",
            json!({
                "path": "/dest.txt"
            }),
        )
        .unwrap();
    assert_eq!(source_content["content"], dest_content["content"]);
}

#[test]
fn test_copy_file_to_nested_path() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create source
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/source.txt",
                "content": "content"
            }),
        )
        .unwrap();

    // Copy to nested path
    sandbox.register_tool(get_copy_file_tool());
    let result = sandbox.invoke_tool(
        "copy_file",
        json!({
            "from": "/source.txt",
            "to": "/nested/path/dest.txt"
        }),
    );
    assert!(result.is_ok(), "copy_file should create parent directories");
}

#[test]
fn test_move_file_basic() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create file
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/original.txt",
                "content": "original content"
            }),
        )
        .unwrap();

    // Move it
    sandbox.register_tool(get_move_file_tool());
    let result = sandbox.invoke_tool(
        "move_file",
        json!({
            "from": "/original.txt",
            "to": "/moved.txt"
        }),
    );
    assert!(result.is_ok());

    // Verify original is gone and destination exists
    sandbox.register_tool(get_file_exists_tool());
    let original_exists = sandbox
        .invoke_tool(
            "file_exists",
            json!({
                "path": "/original.txt"
            }),
        )
        .unwrap();
    let moved_exists = sandbox
        .invoke_tool(
            "file_exists",
            json!({
                "path": "/moved.txt"
            }),
        )
        .unwrap();

    assert_eq!(original_exists["exists"], false);
    assert_eq!(moved_exists["exists"], true);
}

#[test]
fn test_move_file_to_nested_path() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create file
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/file.txt",
                "content": "content"
            }),
        )
        .unwrap();

    // Move to nested path
    sandbox.register_tool(get_move_file_tool());
    let result = sandbox.invoke_tool(
        "move_file",
        json!({
            "from": "/file.txt",
            "to": "/nested/moved/file.txt"
        }),
    );
    assert!(result.is_ok(), "move_file should create parent directories");
}

#[test]
fn test_list_files_basic() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create some files and dirs
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/listtest/file1.txt",
                "content": "content"
            }),
        )
        .unwrap();
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/listtest/file2.txt",
                "content": "content"
            }),
        )
        .unwrap();

    sandbox.register_tool(get_create_directory_tool());
    sandbox
        .invoke_tool(
            "create_directory",
            json!({
                "path": "/listtest/subdir"
            }),
        )
        .unwrap();

    // List files
    sandbox.register_tool(get_list_files_tool());
    let result = sandbox.invoke_tool(
        "list_files",
        json!({
            "path": "/listtest"
        }),
    );
    assert!(result.is_ok());

    let listing = result.unwrap();
    assert!(listing["files"].is_array());
    assert!(listing["directories"].is_array());
}

#[test]
fn test_list_files_empty_directory() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create empty directory
    sandbox.register_tool(get_create_directory_tool());
    sandbox
        .invoke_tool(
            "create_directory",
            json!({
                "path": "/emptydir"
            }),
        )
        .unwrap();

    // List it
    sandbox.register_tool(get_list_files_tool());
    let result = sandbox.invoke_tool(
        "list_files",
        json!({
            "path": "/emptydir"
        }),
    );
    assert!(result.is_ok());
}

#[test]
fn test_create_directory_basic() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_create_directory_tool());

    let result = sandbox.invoke_tool(
        "create_directory",
        json!({
            "path": "/newdir"
        }),
    );
    assert!(result.is_ok());

    // Verify it exists
    sandbox.register_tool(get_file_exists_tool());
    let exists = sandbox
        .invoke_tool(
            "file_exists",
            json!({
                "path": "/newdir"
            }),
        )
        .unwrap();
    assert_eq!(exists["exists"], true);
}

#[test]
fn test_create_directory_nested() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_create_directory_tool());

    let result = sandbox.invoke_tool(
        "create_directory",
        json!({
            "path": "/nested/deep/dir/structure"
        }),
    );
    assert!(
        result.is_ok(),
        "create_directory should create nested paths"
    );
}

#[test]
fn test_delete_directory_basic() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create directory
    sandbox.register_tool(get_create_directory_tool());
    sandbox
        .invoke_tool(
            "create_directory",
            json!({
                "path": "/dirtoremove"
            }),
        )
        .unwrap();

    // Delete it
    sandbox.register_tool(get_delete_directory_tool());
    let result = sandbox.invoke_tool(
        "delete_directory",
        json!({
            "path": "/dirtoremove"
        }),
    );
    assert!(result.is_ok());

    // Verify it's gone
    sandbox.register_tool(get_file_exists_tool());
    let exists = sandbox
        .invoke_tool(
            "file_exists",
            json!({
                "path": "/dirtoremove"
            }),
        )
        .unwrap();
    assert_eq!(exists["exists"], false);
}

#[test]
fn test_delete_directory_recursive() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create nested structure
    sandbox.register_tool(get_create_directory_tool());
    sandbox
        .invoke_tool(
            "create_directory",
            json!({
                "path": "/recursive/nested/deep"
            }),
        )
        .unwrap();

    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/recursive/nested/deep/file.txt",
                "content": "content"
            }),
        )
        .unwrap();

    // Delete entire tree
    sandbox.register_tool(get_delete_directory_tool());
    let result = sandbox.invoke_tool(
        "delete_directory",
        json!({
            "path": "/recursive"
        }),
    );
    assert!(
        result.is_ok(),
        "delete_directory should remove all contents"
    );
}

#[test]
fn test_get_file_info_file() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create file
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/infotest.txt",
                "content": "12345"
            }),
        )
        .unwrap();

    // Get info
    sandbox.register_tool(get_file_info_tool());
    let result = sandbox.invoke_tool(
        "get_file_info",
        json!({
            "path": "/infotest.txt"
        }),
    );
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info["is_file"], true);
    assert_eq!(info["is_dir"], false);
    assert_eq!(info["size"], 5);
}

#[test]
fn test_get_file_info_directory() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create directory
    sandbox.register_tool(get_create_directory_tool());
    sandbox
        .invoke_tool(
            "create_directory",
            json!({
                "path": "/infodir"
            }),
        )
        .unwrap();

    // Get info
    sandbox.register_tool(get_file_info_tool());
    let result = sandbox.invoke_tool(
        "get_file_info",
        json!({
            "path": "/infodir"
        }),
    );
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info["is_file"], false);
    assert_eq!(info["is_dir"], true);
}

#[test]
fn test_file_exists_true() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create file
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/exists_test.txt",
                "content": "content"
            }),
        )
        .unwrap();

    // Check existence
    sandbox.register_tool(get_file_exists_tool());
    let result = sandbox.invoke_tool(
        "file_exists",
        json!({
            "path": "/exists_test.txt"
        }),
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap()["exists"], true);
}

#[test]
fn test_file_exists_false() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_file_exists_tool());

    let result = sandbox.invoke_tool(
        "file_exists",
        json!({
            "path": "/nonexistent.txt"
        }),
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap()["exists"], false);
}

// Helper functions to get tool instances
fn get_read_file_tool() -> Box<dyn Tool> {
    use libagent::tools::ReadFileTool;
    Box::new(ReadFileTool)
}

fn get_write_file_tool() -> Box<dyn Tool> {
    use libagent::tools::WriteFileTool;
    Box::new(WriteFileTool)
}

fn get_append_file_tool() -> Box<dyn Tool> {
    use libagent::tools::AppendFileTool;
    Box::new(AppendFileTool)
}

fn get_delete_file_tool() -> Box<dyn Tool> {
    use libagent::tools::DeleteFileTool;
    Box::new(DeleteFileTool)
}

fn get_copy_file_tool() -> Box<dyn Tool> {
    use libagent::tools::CopyFileTool;
    Box::new(CopyFileTool)
}

fn get_move_file_tool() -> Box<dyn Tool> {
    use libagent::tools::MoveFileTool;
    Box::new(MoveFileTool)
}

fn get_list_files_tool() -> Box<dyn Tool> {
    use libagent::tools::ListFilesTool;
    Box::new(ListFilesTool)
}

fn get_create_directory_tool() -> Box<dyn Tool> {
    use libagent::tools::CreateDirectoryTool;
    Box::new(CreateDirectoryTool)
}

fn get_delete_directory_tool() -> Box<dyn Tool> {
    use libagent::tools::DeleteDirectoryTool;
    Box::new(DeleteDirectoryTool)
}

fn get_file_info_tool() -> Box<dyn Tool> {
    use libagent::tools::GetFileInfoTool;
    Box::new(GetFileInfoTool)
}

fn get_file_exists_tool() -> Box<dyn Tool> {
    use libagent::tools::FileExistsTool;
    Box::new(FileExistsTool)
}
