use libagent::{ResourceLimits, Sandbox};
use nix::unistd;

#[test]
fn sandbox_cannot_see_host_etc() {
    if unistd::geteuid().as_raw() != 0 {
        eprintln!("skipping sandbox_cannot_see_host_etc: not running as root");
        return;
    }
    let sb = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    // run a command that lists /etc inside sandbox
    let etc_listing = sb.run_command("ls -A /etc").unwrap_or_default();
    // host /etc should not appear; sandbox tmpfs starts empty
    assert!(
        etc_listing.trim().is_empty(),
        "sandbox /etc should be empty: {}",
        etc_listing
    );
}

#[test]
fn sandbox_write_does_not_affect_host() {
    if unistd::geteuid().as_raw() != 0 {
        eprintln!("skipping sandbox_write_does_not_affect_host: not running as root");
        return;
    }
    let sb = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sb.run_command("touch /foo").unwrap();
    // file should exist in sandbox root but not on host root
    let inside = sb.root_path().join("foo");
    assert!(inside.exists(), "file should exist inside sandbox");
    assert!(
        !std::path::Path::new("/foo").exists(),
        "host /foo must not exist"
    );
}
