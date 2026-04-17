# MowisAI VM Backend Implementation TODO

## Approved Plan Steps (QEMU-first for Codespace no-KVM, SSH transport, keep guest_backend as dead code)

1. ~~[x] Step 1: Create `scripts/build-rootfs.sh`, run env checks, download/build VM assets (`vmlinux`, `mowis-rootfs.ext4` with /init incl. SSH setup)~~
2. [ ] Step 2: Create `agentd/src/vm_backend.rs` (boot_vm, stop_vm, exec_in_vm_ssh with retry, VmHandle, detect_vm_backend)
3. [ ] Step 3: Edit `agentd/src/lib.rs` (add `pub mod vm_backend;`)
4. [ ] Step 4: Edit `agentd/src/socket_server.rs` (add VM_HANDLES, route create_sandbox/invoke_tool/destroy_sandbox to vm_backend)
5. [ ] Step 5: `cargo build` && `cargo test` (all 52 pass)
6. [ ] Step 6: Add `agentd/tests/vm_backend_test.rs` (#[ignore])
7. [ ] Step 7: E2E test: `./target/debug/agentd orchestrate-interactive --backend guest_vm`
8. [ ] Step 8: Cleanup: deprecate guest_backend.rs

**Rules:** `cargo build`/`cargo test` after every file. QEMU SSH ports 10022+. /init mkdir /root/.ssh chmod 700.

