# New 7-Layer Orchestration: Verification Checklist

Use this checklist to verify the implementation is complete and working.

---

## Phase 1: Code Verification

### Files Created ✅

- [ ] `agentd-protocol/src/lib.rs` - Updated with new types
- [ ] `agentd/src/orchestration/types.rs` - New + legacy types
- [ ] `agentd/src/orchestration/planner.rs` - Layer 1
- [ ] `agentd/src/orchestration/sandbox_topology.rs` - Layer 2
- [ ] `agentd/src/orchestration/scheduler.rs` - Layer 3
- [ ] `agentd/src/orchestration/checkpoint.rs` - Layer 4
- [ ] `agentd/src/orchestration/agent_execution.rs` - Layer 4
- [ ] `agentd/src/orchestration/merge_worker.rs` - Layer 5
- [ ] `agentd/src/orchestration/verification.rs` - Layer 6
- [ ] `agentd/src/orchestration/new_orchestrator.rs` - Layer 7
- [ ] `agentd/src/orchestration/mod.rs` - Updated exports
- [ ] `agentd/src/main.rs` - Added OrchestrateNew command
- [ ] `agentd/Cargo.toml` - Added dependencies

### Stub Files Created ✅

- [ ] `agentd/src/orchestration/architect.rs` - Stub
- [ ] `agentd/src/orchestration/context_gatherer.rs` - Stub
- [ ] `agentd/src/orchestration/coordinator.rs` - Stub
- [ ] `agentd/src/orchestration/sandbox_manager.rs` - Stub
- [ ] `agentd/src/orchestration/sandbox_owner.rs` - Stub

### Documentation Created ✅

- [ ] `NEW_ORCHESTRATION_README.md`
- [ ] `MIGRATION_GUIDE.md`
- [ ] `IMPLEMENTATION_SUMMARY.md`
- [ ] `QUICK_START.md`
- [ ] `VERIFICATION_CHECKLIST.md` (this file)

### Tests Created ✅

- [ ] `agentd/tests/new_orchestration_tests.rs`

---

## Phase 2: Build Verification

Run these commands on your **Linux machine**:

### Compile Check

```bash
cd MowisAI/agentd
cargo check
```

**Expected**: ✅ No errors

**Status**: [ ]

---

### Build Check

```bash
cargo build
```

**Expected**: ✅ Successful build

**Status**: [ ]

---

### Release Build

```bash
cargo build --release
```

**Expected**: ✅ Optimized binary created

**Status**: [ ]

---

## Phase 3: Test Verification

### Run All Tests

```bash
cargo test
```

**Expected**: ✅ 67+ tests passing

**Status**: [ ]

**Number of tests passed**: _______

---

### Run Orchestration Tests Only

```bash
cargo test --test new_orchestration_tests
```

**Expected**: ✅ All new orchestration tests pass

**Status**: [ ]

---

### Run Unit Tests

```bash
cargo test --lib orchestration
```

**Expected**: ✅ All unit tests pass

**Status**: [ ]

---

## Phase 4: Runtime Verification

### Start Socket Server

**Terminal 1**:
```bash
sudo ./target/debug/agentd socket --path /tmp/agentd.sock
```

**Expected**:
```
Socket server listening on /tmp/agentd.sock
```

**Status**: [ ]

---

### Test CLI Help

**Terminal 2**:
```bash
./target/debug/agentd orchestrate-new --help
```

**Expected**: Shows help text with all parameters

**Status**: [ ]

---

### Simple Test Run

```bash
./target/debug/agentd orchestrate-new \
  --prompt "create a hello world function" \
  --project company-internal-tools-490516 \
  --socket /tmp/agentd.sock \
  --project-root . \
  --max-agents 5
```

**Expected**:
- Completes without crashes
- Shows all 7 layers
- Produces output

**Status**: [ ]

---

## Phase 5: Feature Verification

### Layer 1: Fast Planner

**Test**: Does planner generate task graph?

```bash
# Check planner output in orchestration run
# Should see: "Layer 1: Planning tasks..."
# Should see: "→ Generated X tasks across Y sandboxes"
```

**Status**: [ ]

---

### Layer 2: Overlayfs Topology

**Test**: Are overlayfs layers created?

```bash
# During run, check:
ls -la /tmp/mowis-overlay/sandboxes/

# Should see sandbox directories
```

**Status**: [ ]

---

### Layer 3: Scheduler

**Test**: Are tasks dispatched correctly?

```bash
# Check orchestration output
# Should see: "Layer 3: Initializing scheduler..."
# Should see: "→ Scheduler ready with X tasks"
# Should see tasks completing
```

**Status**: [ ]

---

### Layer 4: Agent Execution + Checkpoints

**Test**: Are checkpoints created?

```bash
# During run, check:
ls -la /tmp/mowis-checkpoints/

# Should see agent checkpoint directories
# Should see checkpoint-*.json files
```

**Status**: [ ]

---

### Layer 5: Parallel Merge

**Test**: Are diffs merged?

```bash
# Check orchestration output
# Should see: "Layer 5: Merging agent results per sandbox..."
# Should see: "→ Merging X diffs for sandbox: Y"
# Should see: "✓ Merged with X conflicts resolved"
```

**Status**: [ ]

---

### Layer 6: Verification

**Test**: Are tests generated and run?

```bash
# Check orchestration output
# Should see: "Layer 6: Verifying sandbox results..."
# Should see: "→ Verifying sandbox: X"
# Should see: "✓ Verification: Passed (X rounds)"
```

**Status**: [ ]

---

### Layer 7: Final Output

**Test**: Is final diff produced?

```bash
# Check orchestration output
# Should see: "Layer 7: Final cross-sandbox merge..."
# Should see: "📝 Final merged diff (X bytes)"
# Should see actual diff content
```

**Status**: [ ]

---

## Phase 6: Error Handling Verification

### Tier 1: Tool Retry

**Test**: Simulate tool failure

*Note: This requires manual testing or mocking*

**Expected**: Tool retry up to 3 times

**Status**: [ ]

---

### Tier 2: Agent Crash Recovery

**Test**: Simulate agent crash

*Note: This requires manual testing*

**Expected**: Agent respawns, recovers from checkpoint

**Status**: [ ]

---

### Tier 3: Escalation

**Test**: Simulate repeated failure

*Note: This requires manual testing*

**Expected**: Task marked as failed, full checkpoint log in output

**Status**: [ ]

---

## Phase 7: Performance Verification

### Small Task Performance

**Test**:
```bash
time ./target/debug/agentd orchestrate-new \
  --prompt "add a hello world function" \
  --project company-internal-tools-490516 \
  --socket /tmp/agentd.sock \
  --project-root . \
  --max-agents 10
```

**Expected**: < 1 minute total

**Actual Time**: _______ seconds

**Status**: [ ]

---

### Medium Task Performance

**Test**: Run with 20-50 file project

**Expected**: 1-5 minutes total

**Actual Time**: _______ seconds

**Status**: [ ]

---

## Phase 8: Integration Verification

### Old vs New System Comparison

Run same task with both systems:

**Old System**:
```bash
./target/debug/agentd orchestrate \
  --prompt "implement hello world" \
  --project company-internal-tools-490516 \
  --socket /tmp/agentd.sock \
  --max-agents 10
```

**New System**:
```bash
./target/debug/agentd orchestrate-new \
  --prompt "implement hello world" \
  --project company-internal-tools-490516 \
  --socket /tmp/agentd.sock \
  --project-root . \
  --max-agents 10
```

**Comparison**:
- [ ] New system is faster
- [ ] New system provides more detailed output
- [ ] Both produce similar code quality
- [ ] New system includes verification

---

## Phase 9: Documentation Verification

### README Completeness

- [ ] NEW_ORCHESTRATION_README.md is clear and comprehensive
- [ ] QUICK_START.md provides simple getting started
- [ ] MIGRATION_GUIDE.md explains old → new migration
- [ ] IMPLEMENTATION_SUMMARY.md documents what was built

### Code Documentation

- [ ] All major functions have doc comments
- [ ] Module-level documentation exists
- [ ] Types are well documented

---

## Phase 10: Final Checks

### Hard Invariants (from Architecture Spec)

- [ ] Orchestrator-mediated coordination only (no agent-to-agent)
- [ ] Sandbox/container IDs are String in JSON (never u64)
- [ ] No tests were deleted or modified to pass
- [ ] No stubbed/fake tool implementations
- [ ] All tools execute within container context
- [ ] 67 tests still pass (number never regressed)
- [ ] agentd core unchanged (socket API intact)

### Code Quality

- [ ] No `unwrap()` in production paths
- [ ] Proper error handling with `Result<T>`
- [ ] All `panic!` are in test code only
- [ ] Async/await used correctly with tokio
- [ ] No deadlocks or race conditions

### Resource Cleanup

- [ ] Overlayfs layers are unmounted on cleanup
- [ ] Checkpoint directories are cleaned up
- [ ] Temporary files are removed
- [ ] No resource leaks

---

## Summary

### Overall Status

**Total Checks**: 50+

**Passed**: _______

**Failed**: _______

**Skipped**: _______

**Percentage**: _______ %

---

### Critical Issues Found

List any critical issues:

1. _______________________________________
2. _______________________________________
3. _______________________________________

---

### Non-Critical Issues Found

List any minor issues:

1. _______________________________________
2. _______________________________________
3. _______________________________________

---

### Sign-Off

**Verified By**: _______________________

**Date**: _______________________

**Status**: [ ] ✅ APPROVED [ ] ⚠️ APPROVED WITH CAVEATS [ ] ❌ REJECTED

**Notes**:

_____________________________________________________________

_____________________________________________________________

_____________________________________________________________

---

## Next Steps After Verification

Once all checks pass:

1. **Commit Changes**
   ```bash
   git add .
   git commit -m "Implement new 7-layer orchestration system"
   ```

2. **Create Branch**
   ```bash
   git checkout -b feature/new-orchestration
   git push origin feature/new-orchestration
   ```

3. **Create PR**
   - Review code changes
   - Include IMPLEMENTATION_SUMMARY.md
   - Link to architecture spec

4. **Deploy to Staging**
   - Test on staging environment
   - Run full test suite
   - Performance benchmarks

5. **Production Rollout**
   - Monitor metrics
   - Gradual rollout (10% → 50% → 100%)
   - Keep old system as fallback

---

**Ready for Production**: [ ] YES [ ] NO

**Confidence Level**: [ ] HIGH [ ] MEDIUM [ ] LOW

**Estimated Production Readiness**: _______________________

---

**End of Verification Checklist**
