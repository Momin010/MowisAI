use crate::tools::common::{resolve_path, Tool, ToolContext};
use serde_json::{json, Value};
use std::fs;

// ============== DATA TOOLS (5) ==============

pub struct JsonParseTool;
impl Tool for JsonParseTool {
    fn name(&self) -> &'static str {
        "json_parse"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let data = input["data"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("json_parse: missing data"))?;

        match serde_json::from_str::<Value>(data) {
            Ok(parsed) => Ok(json!({ "parsed": parsed, "success": true })),
            Err(e) => Ok(json!({ "error": e.to_string(), "success": false })),
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(JsonParseTool)
    }
}

pub struct JsonStringifyTool;
impl Tool for JsonStringifyTool {
    fn name(&self) -> &'static str {
        "json_stringify"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let data = &input["data"];
        let string = serde_json::to_string(data)?;
        Ok(json!({ "string": string, "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(JsonStringifyTool)
    }
}

pub struct JsonQueryTool;
impl Tool for JsonQueryTool {
    fn name(&self) -> &'static str {
        "json_query"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let data_str = input["data"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("json_query: missing data"))?;
        let path = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("json_query: missing path"))?;

        let obj: Value = serde_json::from_str(data_str)?;

        let clean = path.trim_start_matches("$.").trim_start_matches("$");
        let mut current = obj.clone();

        for part in clean.split('.') {
            if let Some(bracket) = part.find('[') {
                let key = &part[..bracket];
                let idx_str = &part[bracket + 1..part.len() - 1];
                let idx: usize = idx_str.parse().unwrap_or(0);

                if !key.is_empty() {
                    current = current
                        .get(key)
                        .and_then(|v| v.get(idx))
                        .cloned()
                        .unwrap_or(Value::Null);
                } else {
                    current = current.get(idx).cloned().unwrap_or(Value::Null);
                }
            } else {
                current = current.get(part).cloned().unwrap_or(Value::Null);
            }

            if current.is_null() {
                break;
            }
        }

        Ok(json!({ "result": current, "found": !current.is_null() }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(JsonQueryTool)
    }
}

pub struct CsvReadTool;
impl Tool for CsvReadTool {
    fn name(&self) -> &'static str {
        "csv_read"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("csv_read: missing path"))?;
        let delimiter = input
            .get("delimiter")
            .and_then(|v| v.as_str())
            .unwrap_or(",")
            .chars()
            .next()
            .unwrap_or(',');

        let path = resolve_path(ctx, path_str);
        let content = fs::read_to_string(&path)?;

        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter as u8)
            .from_reader(content.as_bytes());

        let headers: Vec<String> = reader
            .headers()
            .map(|h| h.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default();

        let mut rows = vec![];
        for result in reader.records() {
            let record = result?;
            rows.push(record.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        }

        Ok(json!({ "rows": rows, "headers": headers }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(CsvReadTool)
    }
}

pub struct CsvWriteTool;
impl Tool for CsvWriteTool {
    fn name(&self) -> &'static str {
        "csv_write"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("csv_write: missing path"))?;
        let rows = input["rows"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("csv_write: missing rows"))?;

        let path = resolve_path(ctx, path_str);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut wtr = csv::Writer::from_path(&path)?;
        for row in rows {
            if let Some(arr) = row.as_array() {
                let record: Vec<String> = arr
                    .iter()
                    .map(|v| v.as_str().unwrap_or("").to_string())
                    .collect();
                wtr.write_record(&record)?;
            }
        }
        wtr.flush()?;

        Ok(json!({ "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(CsvWriteTool)
    }
}

// ============== GIT TOOLS (9) ==============
