# Quick Start: New 7-Layer Orchestration

## 1-Minute Quick Start

```bash
# Terminal 1: Start agentd socket (requires root)
sudo cargo run -- socket --path /tmp/agentd.sock

# Terminal 2: Run orchestration
cargo run -- orchestrate-new \
  --prompt "implement user authentication" \
  --project company-internal-tools-490516 \
  --socket /tmp/agentd.sock \
  --project-root .
```

That's it! The system will automatically:
- ✅ Plan tasks (1-3 seconds)
- ✅ Create sandboxes
- ✅ Execute with agents
- ✅ Merge results
- ✅ Verify with tests
- ✅ Output final diff

---

## Prerequisites

### Must Have
- Linux (Ubuntu 20.04+, Debian 11+, etc.)
- Root access (`sudo`)
- Rust toolchain
- gcloud authenticated

### Check Prerequisites
```bash
# Check OS
uname -a  # Should show Linux

# Check Rust
cargo --version  # Should be 1.70+

# Check gcloud
gcloud auth print-access-token  # Should print token
```

---

## Installation

```bash
# Clone repo
git clone https://github.com/mowisai/agentd
cd MowisAI

# Build
cargo build --release

# Run tests
cargo test
# Should see: 67+ tests passing
```

---

## Usage

### Basic Command

```bash
cargo run -- orchestrate-new \
  --prompt "your task description" \
  --project YOUR-GCP-PROJECT-ID \
  --socket /tmp/agentd.sock \
  --project-root /path/to/your/project
```

### With Custom Settings

```bash
cargo run -- orchestrate-new \
  --prompt "implement REST API" \
  --project company-internal-tools-490516 \
  --socket /tmp/agentd.sock \
  --project-root . \
  --overlay-root /tmp/mowis-overlay \
  --checkpoint-root /tmp/mowis-checkpoints \
  --merge-work-dir /tmp/mowis-merge \
  --max-agents 100 \
  --max-verification-rounds 3
```

---

## Examples

### Example 1: Simple Function

```bash
cargo run -- orchestrate-new \
  --prompt "create a hello world function in main.rs" \
  --project company-internal-tools-490516 \
  --socket /tmp/agentd.sock \
  --project-root .
```

**Expected Output**:
- 1-2 tasks
- 1 sandbox
- ~10 seconds total
- Clean git diff

---

### Example 2: Feature Implementation

```bash
cargo run -- orchestrate-new \
  --prompt "implement JWT authentication with login/logout endpoints" \
  --project company-internal-tools-490516 \
  --socket /tmp/agentd.sock \
  --project-root . \
  --max-agents 50
```

**Expected Output**:
- 5-10 tasks
- 2-3 sandboxes (backend, testing)
- ~1-2 minutes total
- Verified with tests

---

### Example 3: Large Refactor

```bash
cargo run -- orchestrate-new \
  --prompt "refactor entire authentication system to use OAuth2" \
  --project company-internal-tools-490516 \
  --socket /tmp/agentd.sock \
  --project-root . \
  --max-agents 200 \
  --max-verification-rounds 3
```

**Expected Output**:
- 20-50 tasks
- 3-5 sandboxes
- ~5-10 minutes total
- Full test coverage

---

## Understanding Output

### Success Output

```
🚀 MowisAI — New 7-Layer Orchestration System
═══════════════════════════════════════════════

Layer 1: Planning tasks...
  → Generated 12 tasks across 3 sandboxes

Layer 2: Creating sandbox topology...
  → Created sandbox: backend

Layer 3: Initializing scheduler...
  → Scheduler ready with 12 tasks

Layer 4: Executing tasks with agents...
  → Executing tasks in sandbox: backend
    ✓ Completed: implement auth module

Layer 5: Merging agent results per sandbox...
  → Merging 4 diffs for sandbox: backend
    ✓ Merged with 2 conflicts resolved

Layer 6: Verifying sandbox results...
  → Verifying sandbox: backend
    ✓ Verification: Passed (1 rounds)

Layer 7: Final cross-sandbox merge...
  → Merging 2 sandbox results

✓ Orchestration complete!
  Total duration: 45s
  Agents used: 12
  Tasks completed: 12/12

Summary: Completed 12/12 tasks using 12 agents in 45s. 0 failed.
```

---

### Partial Success Output

```
⚠️  Failed Tasks:
  task-7 - Tool execution failed after 3 retries

⚠️  Known Issues:
  - backend: test test_auth_flow failed

📝 Final merged diff (8942 bytes)
[diff content]
```

**Action**: Review failed tasks and fix manually

---

## Common Issues

### Issue: "Requires Unix"

**Solution**: Use Linux or WSL2 on Windows

---

### Issue: "Failed to mount overlayfs"

**Solution**: Run with sudo
```bash
sudo cargo run -- orchestrate-new ...
```

---

### Issue: "401 Unauthorized"

**Solution**: Authenticate gcloud
```bash
gcloud auth login
gcloud config set project YOUR-PROJECT-ID
```

---

### Issue: "Socket connection refused"

**Solution**: Start socket server first
```bash
# Terminal 1
sudo cargo run -- socket --path /tmp/agentd.sock
```

---

### Issue: Out of memory

**Solution**: Reduce max_agents
```bash
--max-agents 50  # Instead of 1000
```

---

## Performance Tuning

### Small Tasks (< 5 files)

```bash
--max-agents 10 \
--max-verification-rounds 1
```

⏱️ Expected: < 30 seconds

---

### Medium Tasks (5-50 files)

```bash
--max-agents 50 \
--max-verification-rounds 2
```

⏱️ Expected: 1-3 minutes

---

### Large Tasks (50+ files)

```bash
--max-agents 200 \
--max-verification-rounds 3
```

⏱️ Expected: 5-15 minutes

---

## Next Steps

1. **Try It**: Run a simple task
2. **Read Docs**: See NEW_ORCHESTRATION_README.md
3. **Review Architecture**: See MowisAI_Architecture_Spec.md
4. **Report Issues**: GitHub Issues or engineering@mowis.ai

---

## Help

```bash
# Show all commands
cargo run -- --help

# Show orchestrate-new help
cargo run -- orchestrate-new --help
```

---

**Quick Links**:
- Full README: `NEW_ORCHESTRATION_README.md`
- Architecture: `MowisAI_Architecture_Spec.md`
- Migration: `MIGRATION_GUIDE.md`
- Implementation: `IMPLEMENTATION_SUMMARY.md`

---

**Ready to scale to 1000+ agents? Let's go! 🚀**
