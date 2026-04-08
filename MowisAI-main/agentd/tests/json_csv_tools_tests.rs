use libagent::{ResourceLimits, Sandbox, Tool};
use serde_json::json;

// ============== JSON AND CSV OPERATION TESTS ==============
// json_parse, json_stringify, json_query, csv_read, csv_write

#[test]
fn test_json_parse_valid() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_json_parse_tool());

    let result = sandbox.invoke_tool(
        "json_parse",
        json!({
            "data": r#"{"name": "test", "value": 123}"#
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output["success"], true);
    assert!(output["parsed"].is_object());
}

#[test]
fn test_json_parse_array() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_json_parse_tool());

    let result = sandbox.invoke_tool(
        "json_parse",
        json!({
            "data": r#"[1, 2, 3, 4, 5]"#
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output["success"], true);
    assert!(output["parsed"].is_array());
}

#[test]
fn test_json_parse_nested() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_json_parse_tool());

    let result = sandbox.invoke_tool(
        "json_parse",
        json!({
            "data": r#"{"user": {"name": "John", "age": 30, "address": {"city": "NYC"}}}"#
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output["success"], true);
}

#[test]
fn test_json_parse_invalid() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_json_parse_tool());

    let result = sandbox.invoke_tool(
        "json_parse",
        json!({
            "data": r#"{ invalid json }"#
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output["success"], false);
    assert!(output["error"].is_string());
}

#[test]
fn test_json_parse_missing_data() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_json_parse_tool());

    let result = sandbox.invoke_tool("json_parse", json!({}));
    assert!(result.is_err(), "json_parse without data should fail");
}

#[test]
fn test_json_stringify_object() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_json_stringify_tool());

    let result = sandbox.invoke_tool(
        "json_stringify",
        json!({
            "data": {"name": "test", "value": 123}
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output["success"], true);
    assert!(output["string"].is_string());
    let string_val = output["string"].as_str().unwrap();
    assert!(string_val.contains("name") || string_val.contains("value"));
}

#[test]
fn test_json_stringify_array() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_json_stringify_tool());

    let result = sandbox.invoke_tool(
        "json_stringify",
        json!({
            "data": [1, 2, 3, 4, 5]
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output["success"], true);
    assert!(output["string"].is_string());
}

#[test]
fn test_json_stringify_null() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_json_stringify_tool());

    let result = sandbox.invoke_tool(
        "json_stringify",
        json!({
            "data": null
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert_eq!(output["success"], true);
    assert_eq!(output["string"], "null");
}

#[test]
fn test_json_stringify_primitives() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_json_stringify_tool());

    // String
    let r1 = sandbox.invoke_tool("json_stringify", json!({"data": "test"}));
    assert!(r1.is_ok());

    // Number
    let r2 = sandbox.invoke_tool("json_stringify", json!({"data": 42}));
    assert!(r2.is_ok());

    // Boolean
    let r3 = sandbox.invoke_tool("json_stringify", json!({"data": true}));
    assert!(r3.is_ok());
}

#[test]
fn test_json_query_simple_property() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_json_query_tool());

    let result = sandbox.invoke_tool(
        "json_query",
        json!({
            "data": r#"{"name": "John", "age": 30}"#,
            "path": "$.name"
        }),
    );
    assert!(result.is_ok());
    // Result should contain queried data
}

#[test]
fn test_json_query_nested_property() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_json_query_tool());

    let result = sandbox.invoke_tool(
        "json_query",
        json!({
            "data": r#"{"user": {"name": "John", "address": {"city": "NYC"}}}"#,
            "path": "$.user.address.city"
        }),
    );
    assert!(result.is_ok());
}

#[test]
fn test_json_query_array_index() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_json_query_tool());

    let result = sandbox.invoke_tool(
        "json_query",
        json!({
            "data": r#"{"items": ["a", "b", "c"]}"#,
            "path": "$.items[0]"
        }),
    );
    assert!(result.is_ok());
}

#[test]
fn test_json_query_missing_data() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_json_query_tool());

    let result = sandbox.invoke_tool(
        "json_query",
        json!({
            "path": "$.name"
        }),
    );
    assert!(result.is_err(), "json_query without data should fail");
}

#[test]
fn test_json_query_missing_path() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_json_query_tool());

    let result = sandbox.invoke_tool(
        "json_query",
        json!({
            "data": r#"{"name": "test"}"#
        }),
    );
    assert!(result.is_err(), "json_query without path should fail");
}

#[test]
fn test_csv_write_and_read() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Write CSV
    sandbox.register_tool(get_csv_write_tool());
    let write_result = sandbox.invoke_tool(
        "csv_write",
        json!({
            "path": "/test.csv",
            "rows": [
                ["name", "age", "city"],
                ["John", "30", "NYC"],
                ["Jane", "28", "LA"],
                ["Bob", "35", "Chicago"]
            ]
        }),
    );
    assert!(write_result.is_ok());

    // Read it back
    sandbox.register_tool(get_csv_read_tool());
    let read_result = sandbox.invoke_tool(
        "csv_read",
        json!({
            "path": "/test.csv"
        }),
    );
    assert!(read_result.is_ok());
    let output = read_result.unwrap();
    assert!(output["rows"].is_array());
    assert!(output["headers"].is_array());
}

#[test]
fn test_csv_read_missing_file() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_csv_read_tool());

    let result = sandbox.invoke_tool(
        "csv_read",
        json!({
            "path": "/nonexistent.csv"
        }),
    );
    assert!(result.is_err(), "reading non-existent CSV should fail");
}

#[test]
fn test_csv_read_missing_path() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_csv_read_tool());

    let result = sandbox.invoke_tool("csv_read", json!({}));
    assert!(result.is_err(), "csv_read without path should fail");
}

#[test]
fn test_csv_write_missing_path() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_csv_write_tool());

    let result = sandbox.invoke_tool(
        "csv_write",
        json!({
            "rows": [["a", "b"], ["c", "d"]]
        }),
    );
    assert!(result.is_err(), "csv_write without path should fail");
}

#[test]
fn test_csv_write_missing_rows() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_csv_write_tool());

    let result = sandbox.invoke_tool(
        "csv_write",
        json!({
            "path": "/test.csv"
        }),
    );
    assert!(result.is_err(), "csv_write without rows should fail");
}

#[test]
fn test_csv_write_empty_rows() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_csv_write_tool());

    let result = sandbox.invoke_tool(
        "csv_write",
        json!({
            "path": "/empty.csv",
            "rows": []
        }),
    );
    assert!(result.is_ok());
}

#[test]
fn test_csv_write_single_row() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_csv_write_tool());

    let result = sandbox.invoke_tool(
        "csv_write",
        json!({
            "path": "/single.csv",
            "rows": [["name", "age"]]
        }),
    );
    assert!(result.is_ok());
}

#[test]
fn test_csv_write_nested_paths() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sandbox.register_tool(get_csv_write_tool());

    let result = sandbox.invoke_tool(
        "csv_write",
        json!({
            "path": "/nested/deep/file.csv",
            "rows": [["data"]]
        }),
    );
    assert!(result.is_ok(), "csv_write should create parent directories");
}

#[test]
fn test_csv_read_custom_delimiter() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create a TSV file (tab-delimited)
    sandbox.register_tool(get_write_file_tool());
    sandbox
        .invoke_tool(
            "write_file",
            json!({
                "path": "/test.tsv",
                "content": "name\tage\tcity\nJohn\t30\tNYC\nJane\t28\tLA"
            }),
        )
        .unwrap();

    // Read with custom delimiter
    sandbox.register_tool(get_csv_read_tool());
    let result = sandbox.invoke_tool(
        "csv_read",
        json!({
            "path": "/test.tsv",
            "delimiter": "\t"
        }),
    );
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output["rows"].is_array());
}

#[test]
fn test_json_and_csv_integration() {
    let mut sandbox = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();

    // Create JSON data
    let json_data = json!({
        "people": [
            {"name": "John", "age": 30},
            {"name": "Jane", "age": 28}
        ]
    });

    // Stringify it
    sandbox.register_tool(get_json_stringify_tool());
    let stringify_result = sandbox.invoke_tool(
        "json_stringify",
        json!({
            "data": json_data
        }),
    );
    assert!(stringify_result.is_ok());

    // Parse it back
    let json_string = stringify_result.unwrap()["string"]
        .as_str()
        .unwrap()
        .to_string();
    sandbox.register_tool(get_json_parse_tool());
    let parse_result = sandbox.invoke_tool(
        "json_parse",
        json!({
            "data": json_string
        }),
    );
    assert!(parse_result.is_ok());
}

// Helper functions to get tool instances
fn get_json_parse_tool() -> Box<dyn Tool> {
    use libagent::tools::JsonParseTool;
    Box::new(JsonParseTool)
}

fn get_json_stringify_tool() -> Box<dyn Tool> {
    use libagent::tools::JsonStringifyTool;
    Box::new(JsonStringifyTool)
}

fn get_json_query_tool() -> Box<dyn Tool> {
    use libagent::tools::JsonQueryTool;
    Box::new(JsonQueryTool)
}

fn get_csv_read_tool() -> Box<dyn Tool> {
    use libagent::tools::CsvReadTool;
    Box::new(CsvReadTool)
}

fn get_csv_write_tool() -> Box<dyn Tool> {
    use libagent::tools::CsvWriteTool;
    Box::new(CsvWriteTool)
}

fn get_write_file_tool() -> Box<dyn Tool> {
    use libagent::tools::WriteFileTool;
    Box::new(WriteFileTool)
}
