use libagent::{ResourceLimits, Sandbox};
use std::fs;
use std::path::Path;

#[test]
fn sandbox_ids_increment() {
    let s1 = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    let s2 = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    assert!(s2.id() > s1.id(), "sandbox ids should increase");
}

#[test]
fn child_limits_are_clamped() {
    let parent = Sandbox::new(ResourceLimits {
        ram_bytes: Some(1024),
        cpu_millis: Some(100),
    })
    .unwrap();
    let requested = ResourceLimits {
        ram_bytes: Some(2048),
        cpu_millis: Some(500),
    };
    let child = parent.spawn_child(requested).unwrap();
    assert_eq!(child.limits().ram_bytes, Some(1024));
    assert_eq!(child.limits().cpu_millis, Some(100));
}

#[test]
fn cgroup_limits_written_when_root() {
    if nix::unistd::geteuid().as_raw() != 0 {
        eprintln!("skipping cgroup_limits_written_when_root: not root");
        return;
    }
    let orig = Sandbox::new(ResourceLimits {
        ram_bytes: Some(4096),
        cpu_millis: Some(250),
    })
    .unwrap();
    let cg = Path::new("/sys/fs/cgroup/agentd").join(format!("sandbox-{}", orig.id()));
    assert!(cg.exists(), "cgroup directory should exist");
    let mem = fs::read_to_string(cg.join("memory.max")).unwrap();
    assert!(mem.trim() == "4096");
}
