# Migration Guide: Old 5-Layer → New 7-Layer Orchestration

## Overview

This guide helps you migrate from the deprecated 5-layer orchestration system to the new 7-layer architecture.

---

## Key Differences

| Aspect | Old System | New System |
|--------|------------|------------|
| **Planning** | Context Gatherer (up to 128 rounds) + Architect + Sandbox Owner | Fast Planner (1 LLM call) |
| **Task Dispatch** | Batch-based | Event-driven |
| **Merge** | Serial (single container) | Parallel (tree-pattern) |
| **Reliability** | No checkpoints | Checkpoint after every tool call |
| **Verification** | Manual | Automated with fix re-injection |
| **Scale Target** | ~50 agents | 1000+ agents |

---

## Command Line Changes

### Old Command

```bash
cargo run -- orchestrate \
  --prompt "your task" \
  --project company-internal-tools-490516 \
  --socket /tmp/agentd.sock \
  --max-agents 10 \
  --debug
```

### New Command

```bash
cargo run -- orchestrate-new \
  --prompt "your task" \
  --project company-internal-tools-490516 \
  --socket /tmp/agentd.sock \
  --project-root . \
  --overlay-root /tmp/mowis-overlay \
  --checkpoint-root /tmp/mowis-checkpoints \
  --merge-work-dir /tmp/mowis-merge \
  --max-agents 1000 \
  --max-verification-rounds 3
```

### New Required Parameters

- `--project-root`: Path to your project (default: `.`)
- `--overlay-root`: Overlayfs mount directory (default: `/tmp/mowis-overlay`)
- `--checkpoint-root`: Checkpoint storage directory (default: `/tmp/mowis-checkpoints`)
- `--merge-work-dir`: Merge workspace directory (default: `/tmp/mowis-merge`)
- `--max-verification-rounds`: Max test verification rounds (default: `3`)

### Removed Parameters

- `--debug`: Replaced with standard logging (set `RUST_LOG=debug`)

---

## API Changes

### Old API

```rust
use libagent::orchestration::orchestrator;

orchestrator::run(
    "implement user auth",
    "company-internal-tools-490516",
    "/tmp/agentd.sock",
    10,
)?;
```

### New API

```rust
use libagent::orchestration::{NewOrchestrator, OrchestratorConfig};

let config = OrchestratorConfig {
    project_id: "company-internal-tools-490516".to_string(),
    socket_path: "/tmp/agentd.sock".to_string(),
    project_root: PathBuf::from("."),
    overlay_root: PathBuf::from("/tmp/mowis-overlay"),
    checkpoint_root: PathBuf::from("/tmp/mowis-checkpoints"),
    merge_work_dir: PathBuf::from("/tmp/mowis-merge"),
    max_agents: 1000,
    max_verification_rounds: 3,
};

let orchestrator = NewOrchestrator::new(config);

let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .build()?;

let output = runtime.block_on(orchestrator.run("implement user auth"))?;

println!("Summary: {}", output.summary);
println!("Total agents: {}", output.total_agents_used);
println!("Duration: {}s", output.total_duration_secs);
```

---

## Output Format Changes

### Old Output

```
Starting orchestration...
Context gathering complete.
Architecture planning complete.
Executing in 2 sandboxes...
Sandbox 1: backend (3 workers)
Sandbox 2: frontend (2 workers)
Merging results...
Done.
```

### New Output

```
🚀 MowisAI — New 7-Layer Orchestration System
═══════════════════════════════════════════════

Layer 1: Planning tasks...
  → Generated 12 tasks across 3 sandboxes

Layer 2: Creating sandbox topology...
  → Created sandbox: backend
  → Created sandbox: frontend
  → Created sandbox: testing

Layer 3: Initializing scheduler...
  → Scheduler ready with 12 tasks

Layer 4: Executing tasks with agents...
  → Executing tasks in sandbox: backend
    ✓ Completed: implement JWT auth module
    ✓ Completed: add login endpoint
  → Executing tasks in sandbox: frontend
    ✓ Completed: build login form
  → Completed: 12/12 tasks

Layer 5: Merging agent results per sandbox...
  → Merging 4 diffs for sandbox: backend
    ✓ Merged with 2 conflicts resolved
  → Merging 3 diffs for sandbox: frontend
    ✓ Merged with 0 conflicts resolved

Layer 6: Verifying sandbox results...
  → Verifying sandbox: backend
    ✓ Verification: Passed (1 rounds)
  → Verifying sandbox: frontend
    ✓ Verification: Passed (1 rounds)

Layer 7: Final cross-sandbox merge...
  → Merging 2 sandbox results

✓ Orchestration complete!
  Total duration: 45s
  Agents used: 12
  Tasks completed: 12/12

═══════════════════════════════════════════════
📊 FINAL RESULTS
═══════════════════════════════════════════════

Summary: Completed 12/12 tasks using 12 agents in 45s. 0 failed.

Scheduler Stats:
  Total tasks: 12
  Completed: 12
  Failed: 0
  Running: 0
  Pending: 0

Sandbox Results:
  backend - Passed
  frontend - Passed
  testing - Passed

✅ Orchestration complete!
```

---

## Behavior Changes

### 1. Planning Phase

**Old**: Could take 2-5 minutes with multiple LLM calls

**New**: Completes in 1-3 seconds with single LLM call

**Action Required**: None - faster is better!

---

### 2. Error Handling

**Old**: Agent crash = task failure, no recovery

**New**: 3-tier error handling with automatic recovery

**Action Required**:
- Review failed tasks in output
- Check `failed_tasks` field in `FinalOutput`
- Failed tasks now include full checkpoint log for debugging

---

### 3. Verification

**Old**: No automated testing

**New**: Automated verification with test generation and fix re-injection

**Action Required**:
- Set `max_verification_rounds` based on project size
- Review `known_issues` field for failing tests
- Check `verification_status` per sandbox

---

### 4. Merge Strategy

**Old**: Serial merge in single container

**New**: Parallel tree-pattern merge with LLM conflict repair

**Action Required**:
- Expect faster merge times for many agents
- Review `conflicts_resolved` count in merge results

---

## File Structure Changes

### Old Structure

```
orchestration/
├── context_gatherer.rs  ← DEPRECATED
├── architect.rs         ← DEPRECATED
├── sandbox_owner.rs     ← DEPRECATED
├── sandbox_manager.rs   ← DEPRECATED
├── worker.rs            ← DEPRECATED
├── coordinator.rs       ← DEPRECATED
├── executor.rs          ← DEPRECATED
├── planner.rs (old)     ← DEPRECATED
└── orchestrator.rs      ← DEPRECATED
```

### New Structure

```
orchestration/
├── types.rs              ← NEW
├── planner.rs            ← NEW (Layer 1)
├── sandbox_topology.rs   ← NEW (Layer 2)
├── scheduler.rs          ← NEW (Layer 3)
├── checkpoint.rs         ← NEW (Layer 4)
├── agent_execution.rs    ← NEW (Layer 4)
├── merge_worker.rs       ← NEW (Layer 5)
├── verification.rs       ← NEW (Layer 6)
├── new_orchestrator.rs   ← NEW (Layer 7)
├── session_store.rs      ← KEPT
├── sandbox_profiles.rs   ← KEPT
└── [deprecated stubs]
```

---

## Deprecated Features

The following features from the old system are **not available** in the new system:

### 1. Interactive REPL Mode

**Old**: `orchestrate-interactive` command with session persistence

**New**: Not yet implemented

**Workaround**: Use single-shot `orchestrate-new` command for now. Interactive mode will be added in a future release.

---

### 2. Session File Persistence

**Old**: `--session-file` to save/resume interactive sessions

**New**: Not yet implemented

**Workaround**: Use checkpoint logs for debugging. Session persistence coming soon.

---

### 3. Custom Context Gathering

**Old**: Could provide custom context files

**New**: Automatic via directory scan

**Workaround**: Structure your project with clear directory layout. The planner will automatically detect structure.

---

## Troubleshooting

### "Requires Unix" Error

**Symptom**:
```
Error: New orchestration requires Unix (overlayfs, chroot)
```

**Cause**: Running on Windows without Linux subsystem

**Solution**:
- Use WSL2 on Windows
- Run on Linux server
- Use Docker container with Linux

---

### "Failed to mount overlayfs" Error

**Symptom**:
```
Error: Failed to mount overlayfs: Operation not permitted
```

**Cause**: Not running with root privileges

**Solution**:
```bash
sudo cargo run -- orchestrate-new ...
```

---

### "Gemini API error: 401 Unauthorized"

**Symptom**:
```
Error: Gemini API error: 401 Unauthorized
```

**Cause**: gcloud not authenticated

**Solution**:
```bash
gcloud auth login
gcloud config set project company-internal-tools-490516
```

---

### Slow Performance

**Symptom**: Orchestration takes longer than expected

**Possible Causes**:
1. Network latency to Gemini API
2. Large project with many files
3. Disk I/O bottleneck for checkpoints

**Solutions**:
1. Check network connection
2. Use `.gitignore`-style patterns to limit scanned files
3. Use fast SSD for checkpoint storage
4. Increase `max_agents` to parallelize more

---

### Out of Memory

**Symptom**: System runs out of RAM

**Cause**: Too many concurrent agents

**Solution**:
```bash
# Reduce max_agents
cargo run -- orchestrate-new \
  --max-agents 100 \  # Instead of 1000
  ...
```

---

## Performance Tuning

### Small Projects (< 20 files)

```bash
--max-agents 10 \
--max-verification-rounds 1
```

Expected: < 30 seconds total

---

### Medium Projects (20-100 files)

```bash
--max-agents 50 \
--max-verification-rounds 2
```

Expected: 1-3 minutes total

---

### Large Projects (100+ files)

```bash
--max-agents 200 \
--max-verification-rounds 3
```

Expected: 3-10 minutes total

---

### Very Large Projects (500+ files)

```bash
--max-agents 1000 \
--max-verification-rounds 3
```

Expected: 10-30 minutes total

Resource requirements:
- RAM: ~50GB
- CPU: ~100 cores
- Disk: ~10GB for checkpoints

---

## Testing Your Migration

### Step 1: Run Both Systems Side-by-Side

```bash
# Terminal 1: Old system
cargo run -- orchestrate \
  --prompt "add hello world function" \
  --project company-internal-tools-490516 \
  --socket /tmp/agentd.sock \
  --max-agents 10

# Terminal 2: New system
cargo run -- orchestrate-new \
  --prompt "add hello world function" \
  --project company-internal-tools-490516 \
  --socket /tmp/agentd.sock \
  --max-agents 10
```

### Step 2: Compare Outputs

Check:
- ✅ Both produce similar code changes
- ✅ New system is faster
- ✅ New system provides more detailed progress
- ✅ New system includes verification results

### Step 3: Validate Quality

Run tests on both outputs:
```bash
cargo test
npm test
pytest
```

### Step 4: Full Migration

Once validated, switch all scripts to use `orchestrate-new`.

---

## Rollback Plan

If you need to rollback to the old system:

1. The old system is still available via `orchestrate` command
2. Old code is preserved in deprecated stub files
3. No data migration needed - systems are independent

```bash
# Continue using old system
cargo run -- orchestrate ...
```

---

## Timeline

| Phase | Status | Notes |
|-------|--------|-------|
| Core implementation | ✅ Complete | All 7 layers functional |
| Testing | ✅ Complete | 67 tests passing + new orchestration tests |
| Documentation | ✅ Complete | README + Architecture spec |
| Old system deprecation | ⏳ In Progress | Will be removed in v2.0 |
| Interactive mode | 🔜 Planned | Target: v1.1 |
| Session persistence | 🔜 Planned | Target: v1.1 |

---

## Getting Help

- **Documentation**: See `NEW_ORCHESTRATION_README.md`
- **Architecture**: See `MowisAI_Architecture_Spec.md`
- **Issues**: [GitHub Issues](https://github.com/mowisai/agentd/issues)
- **Email**: engineering@mowis.ai

---

## Feedback

We want to hear from you! Please report:

- ✅ What works well
- ⚠️ What's confusing
- 🐛 Bugs encountered
- 💡 Feature requests

File issues at: https://github.com/mowisai/agentd/issues

---

**Last Updated**: 2026-04-02
**Version**: 1.0.0
**Status**: Production Ready
