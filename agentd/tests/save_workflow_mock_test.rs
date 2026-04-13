//! Mock test for save workflow - no socket server or LLM needed
//!
//! This test verifies the staging + export mechanism works correctly
//! by simulating the full orchestration flow with mock agents.

use std::path::PathBuf;
use std::fs;

/// Creates mock agent workspace with files
fn create_mock_workspace(agent_id: &str, base_dir: &PathBuf) -> PathBuf {
    let workspace = base_dir
        .join(format!("container-{}", agent_id))
        .join("root")
        .join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    
    // Each agent creates unique files
    let agent_dir = workspace.join(format!("agent_{}", agent_id));
    fs::create_dir_all(&agent_dir).unwrap();
    
    fs::write(
        agent_dir.join("main.rs"),
        format!("// Agent {}\nfn main() {{ println!(\"{}\"); }}\n", agent_id, agent_id)
    ).unwrap();
    
    fs::write(
        agent_dir.join("lib.rs"),
        format!("// Lib from {}\npub fn add(a: i32, b: i32) -> i32 {{ a + b }}\n", agent_id)
    ).unwrap();
    
    agent_dir
}

/// Simulates staging (copy workspace to staging dir)
fn stage_workspace(agent_id: &str, container_root: &PathBuf, staging_root: &PathBuf) -> PathBuf {
    let src = container_root
        .join(format!("container-{}", agent_id))
        .join("root")
        .join("workspace");
    let dst = staging_root.join(format!("staged-agent-{}", agent_id));
    
    fs::create_dir_all(&dst).unwrap();
    copy_recursive(&src, &dst).unwrap();
    
    println!("  📦 Staged agent {} -> {}", agent_id, dst.display());
    dst
}

/// Simulates export (copy staging to output)
fn export_staged(staging_root: &PathBuf, output_dir: &PathBuf) -> (usize, usize, usize) {
    let mut containers = 0;
    let mut workspaces = 0;
    let mut files = 0;
    
    for entry in fs::read_dir(staging_root).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = path.file_name().unwrap().to_str().unwrap();
        
        if !name.starts_with("staged-") || !path.is_dir() {
            continue;
        }
        
        containers += 1;
        let file_count = copy_recursive(&path, output_dir).unwrap();
        workspaces += 1;
        files += file_count;
        
        println!("  ✅ Exported {} ({} files)", name, file_count);
    }
    
    (containers, workspaces, files)
}

fn copy_recursive(src: &PathBuf, dst: &PathBuf) -> Result<usize, std::io::Error> {
    let mut count = 0;
    
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        
        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)?;
            count += copy_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
            count += 1;
        }
    }
    
    Ok(count)
}

fn list_files(dir: &PathBuf, indent: usize) {
    let prefix = "  ".repeat(indent);
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            println!("{}📁 {}/", prefix, name.to_str().unwrap());
            list_files(&path, indent + 1);
        } else {
            let size = fs::metadata(&path).unwrap().len();
            println!("{}📄 {} ({} bytes)", prefix, name.to_str().unwrap(), size);
        }
    }
}

/// Full end-to-end mock test
#[test]
fn test_save_workflow_mock() {
    println!("\n══════════════════════════════════════════════════");
    println!("🧪 MOCK SAVE WORKFLOW TEST");
    println!("══════════════════════════════════════════════════\n");
    
    // Setup
    let base = std::env::temp_dir().join("mowis-mock").join(format!("{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    
    let containers = base.join("containers");
    let staging = base.join("staging");
    let output = base.join("output");
    
    fs::create_dir_all(&containers).unwrap();
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(&output).unwrap();
    
    // Step 1: Agents create workspaces
    println!("Step 1: Agents creating workspaces...");
    let agents = vec!["alpha", "beta", "gamma"];
    for agent in &agents {
        create_mock_workspace(agent, &containers);
        println!("  → Agent {} created workspace", agent);
    }
    
    // Step 2: Stage workspaces BEFORE destroy
    println!("\nStep 2: Staging workspaces (BEFORE destroy)...");
    for agent in &agents {
        stage_workspace(agent, &containers, &staging);
    }
    
    // Step 3: Simulate destroy (containers "gone")
    println!("\nStep 3: Simulating container destruction...");
    println!("  → Original workspaces now inaccessible");
    
    // Step 4: Export staged to output
    println!("\nStep 4: Exporting staged workspaces...");
    let (found, copied, files) = export_staged(&staging, &output);
    
    println!("\n📊 Results:");
    println!("  Containers found: {}", found);
    println!("  Workspaces copied: {}", copied);
    println!("  Files copied: {}", files);
    
    // Verify
    assert_eq!(found, 3, "Should find 3 staged containers");
    assert_eq!(copied, 3, "Should copy all 3 workspaces");
    assert!(files >= 6, "Should copy at least 6 files");
    
    // Verify content
    assert!(output.join("agent_alpha").exists());
    assert!(output.join("agent_beta").exists());
    assert!(output.join("agent_gamma").exists());
    
    let alpha_main = fs::read_to_string(output.join("agent_alpha").join("main.rs")).unwrap();
    assert!(alpha_main.contains("alpha"), "Alpha content preserved");
    
    let beta_lib = fs::read_to_string(output.join("agent_beta").join("lib.rs")).unwrap();
    assert!(beta_lib.contains("beta"), "Beta content preserved");
    
    // Show final structure
    println!("\n📂 Output structure:");
    list_files(&output, 0);
    
    // Cleanup
    let _ = fs::remove_dir_all(&base);
    
    println!("\n══════════════════════════════════════════════════");
    println!("✅ SAVE WORKFLOW TEST PASSED!");
    println!("══════════════════════════════════════════════════\n");
}

/// Test the actual libagent export function with mock data
#[test]
fn test_libagent_export_function() {
    println!("\n══════════════════════════════════════════════════");
    println!("🧪 LIBAGENT EXPORT FUNCTION TEST");
    println!("══════════════════════════════════════════════════\n");
    
    let base = std::env::temp_dir().join("mowis-lib").join(format!("{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    
    let staging = base.join("staging");
    let output = base.join("output");
    
    fs::create_dir_all(&staging).unwrap();
    fs::create_dir_all(&output).unwrap();
    
    // Create mock staged workspaces
    for i in 0..3 {
        let dir = staging.join(format!("staged-agent-{}", i));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("file{}.txt", i)), format!("Content {}", i)).unwrap();
        
        let subdir = dir.join("src");
        fs::create_dir_all(&subdir).unwrap();
        fs::write(subdir.join("code.rs"), format!("// Code {}", i)).unwrap();
    }
    
    // Use actual libagent function
    let summary = libagent::orchestration::sandbox_topology::export_staged_workspaces_from_dir(
        &staging,
        &output,
    ).unwrap();
    
    println!("Export result: {:?}", summary);
    
    assert_eq!(summary.containers_found, 3);
    assert_eq!(summary.workspaces_copied, 3);
    assert_eq!(summary.files_copied, 6); // 2 files per agent
    
    // Verify
    assert!(output.join("file0.txt").exists());
    assert!(output.join("file1.txt").exists());
    assert!(output.join("file2.txt").exists());
    assert!(output.join("src").join("code.rs").exists());
    
    let _ = fs::remove_dir_all(&base);
    
    println!("\n══════════════════════════════════════════════════");
    println!("✅ LIBAGENT EXPORT TEST PASSED!");
    println!("══════════════════════════════════════════════════\n");
}
