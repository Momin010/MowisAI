# Socket Server Lifecycle Management - Testing Guide

## Summary of Changes

### Files Modified:
1. **agentd/Cargo.toml** - Already has `signal-hook = "0.3"`
2. **agentd/src/lib.rs** - Added socket management utilities:
   - `socket_is_responsive()` - Check if socket server is running
   - `get_socket_server_pid()` - Get PID using pgrep
   - `save_socket_pid()` - Save PID to file
   - `read_socket_pid()` - Read PID from file
   - `start_socket_server_daemon()` - Start socket server with sudo

3. **agentd/src/main.rs** - Updated to use public socket functions from lib.rs:
   - `ensure_socket_server()` - Now delegates to `start_socket_server_daemon()`
   - Removed duplicate functions in main.rs

4. **agentd/src/tui/mod.rs** - Graceful shutdown handling:
   - Check for shutdown flag in event loop
   - Print socket server PID on exit

5. **agentd/src/tui/app.rs** - Socket server commands:
   - `/quit` - Kills socket server and exits
   - `/kill-socket` - Explicitly kill socket server
   - `/socket status` - Show socket server status
   - `/socket restart` - Kill and restart socket server
   - `/launch` - Start socket server or connect to existing one
   - Updated `/help` to show new commands

### Signal Handlers:
- SIGINT (Ctrl+C) → Graceful TUI shutdown (socket server stays alive)
- SIGTERM (kill command) → Graceful TUI shutdown (socket server stays alive)
- `/quit` command → Kill socket server and exit mowisai

---

## Build Instructions

```bash
cd /workspaces/MowisAI/MowisAI-main
cargo build --release
sudo cp target/release/mowisai /usr/local/bin/
```

---

## Test Scenarios

### Test 1: First Run - Socket Server Auto-Start with PID Tracking

**Setup:**
```bash
# Delete old PID file if it exists
rm -f ~/.mowisai/.socket-server.pid

# Kill any existing socket servers
pkill -f "agentd.*socket" || true
rm -f /tmp/mowisai.sock
```

**Steps:**
1. Terminal A: `mowisai`
2. Note the socket PID displayed on startup
3. Type `/socket status`
4. Verify: Status shows "RUNNING ✓" with correct PID

**Expected Result:**
- Socket server starts automatically
- PID saved to ~/.mowisai/.socket-server.pid
- `/socket status` shows: "RUNNING ✓"

---

### Test 2: Ctrl+C Keeps Socket Server Alive

**Prerequisites:** Socket server is running (from Test 1)

**Steps:**
1. Terminal A (with mowisai running): Press Ctrl+C
2. Verify printed message: "Socket server continues with PID: X"
3. Terminal B: `ps aux | grep agentd`
4. Verify: agentd socket process is still running
5. Terminal A: `mowisai` again
6. Verify: Connects to existing socket (message should indicate this)

**Expected Result:**
- Ctrl+C closes mowisai but socket server stays alive
- Same socket server continues to run
- Restart mowisai connects to existing socket
- No new socket server is created

---

### Test 3: /quit Command Kills Everything

**Prerequisites:** Socket server is running

**Steps:**
1. Terminal A (with mowisai running): Type `/quit`
2. Verify: Message shows "Everything stopped. Goodbye!"
3. Terminal B: `ps aux | grep agentd`
4. Verify: agentd socket process is NOT running
5. Terminal A: `mowisai` again
6. Verify: Creates NEW socket server with different PID

**Expected Result:**
- `/quit` kills socket server and exits mowisai
- Next run creates a brand new socket server
- All cleanup is complete

---

### Test 4: /kill-socket Explicit Kill

**Prerequisites:** Socket server is running

**Steps:**
1. Terminal A: Type `/kill-socket`
2. Verify: Message shows "Socket server stopped. Run /launch to restart."
3. Verify: TUI is still running (mowisai didn't exit)
4. Terminal B: `ps aux | grep agentd`
5. Verify: agentd socket process is NOT running
6. Terminal A: Type `/socket status`
7. Verify: Status shows "STOPPED ✗"
8. Terminal A: Type `/launch`
9. Verify: New socket server starts, message shows "Socket server started successfully"

**Expected Result:**
- `/kill-socket` kills socket server but mowisai stays open
- TUI remains responsive
- `/launch` can restart socket server
- `/socket status` reflects correct state

---

### Test 5: External kill Keeps Socket Server Alive

**Prerequisites:** Socket server is running

**Steps:**
1. Terminal A: `mowisai` and note the PID (e.g., 5000)
2. Terminal B: `kill 5000`
3. Terminal A: Should show "Socket server continues with PID: X" and exit
4. Terminal B: `ps aux | grep agentd`
5. Verify: agentd socket process is still running
6. Terminal A: `mowisai` again
7. Verify: Connects to existing socket server

**Expected Result:**
- `kill` command from external source closes mowisai
- Socket server stays alive (separate process)
- Next mowisai run connects to existing socket
- User can kill just mowisai without killing the infrastructure

---

### Test 6: /socket restart Command

**Prerequisites:** Socket server is running

**Steps:**
1. Terminal A: Note current socket PID from `/socket status`
2. Terminal A: Type `/socket restart`
3. Verify: Message shows "✓ Socket server restarted successfully (PID: Y)"
4. Verify: New PID is different from old PID
5. Terminal B: `ps aux | grep agentd`
6. Verify: Only one agentd socket process is running
7. Terminal A: Type `/socket status`
8. Verify: Status shows new PID

**Expected Result:**
- `/socket restart` kills old socket and starts new one
- No orphaned processes left behind
- Seamless restart without interrupting TUI
- New socket PID is tracked

---

### Test 7: /launch Command (Recovery)

**Prerequisites:** Socket server is dead

**Steps:**
1. Terminal A: `pkill -f "agentd.*socket"` to kill socket server
2. Terminal A: In mowisai, type `/launch`
3. Verify: Message shows "🚀 Socket server started successfully (PID: X)"
4. Terminal A: Type `/socket status`
5. Verify: Status shows "RUNNING ✓"

**Expected Result:**
- `/launch` can recover from dead socket server
- Automatically starts new socket daemon
- Tracks PID correctly
- Seamless recovery

---

## Verification Checklist

After running all tests, verify:

- [ ] Socket server starts automatically on first run
- [ ] Ctrl+C preserves socket server
- [ ] `/quit` kills everything cleanly
- [ ] `/kill-socket` works without exiting mowisai
- [ ] External `kill` preserves socket server
- [ ] `/socket restart` creates new process with different PID
- [ ] `/launch` can start or connect to socket server
- [ ] No orphaned processes after any operation
- [ ] PID file is created/updated correctly
- [ ] Socket connection state is always accurate
- [ ] All commands work in any order

---

## Debugging Commands

If tests fail, use these commands to diagnose:

```bash
# Check socket server status
ps aux | grep agentd

# Check socket responsiveness
ss -x | grep mowisai

# Read current PID file
cat ~/.mowisai/.socket-server.pid

# Check mowisai log file
tail -f ~/.mowisai/mowisai.log

# Manually kill socket server
pkill -f "agentd.*socket"

# Clean up for fresh test
rm -f /tmp/mowisai.sock ~/.mowisai/.socket-server.pid
```

---

## Expected Behavior Summary

| Action | Socket Server | TUI | Result |
|--------|--------------|-----|--------|
| Ctrl+C | **STAYS ALIVE** ✓ | Closes | Socket continues in background |
| `/quit` | **DIES** ✗ | Closes | Everything stops cleanly |
| `/kill-socket` | **DIES** ✗ | Stays open | Can restart with `/launch` |
| `/socket restart` | **DIES + RESTARTS** | Stays open | Fresh socket process |
| `kill <mowisai_pid>` (external) | **STAYS ALIVE** ✓ | Closes | Socket continues independently |
| `/launch` (when dead) | **STARTS** | Stays open | Recovery without restart |

---

## Build & Release

When tests pass:

```bash
# Build release version
cargo build --release

# Install globally
sudo cp target/release/mowisai /usr/local/bin/
sudo cp target/release/agentd /usr/local/bin/

# Run final verification
which mowisai
which agentd
mowisai --version
```

---

## Known Limitations

1. Socket server requires sudo privileges for overlayfs operations
2. Non-interactive sudo (-n flag) requires pre-cached sudo credentials
3. If sudo credentials expire, user will be prompted for password
4. Socket path must be writable by non-root user or daemon process
5. PID file is stored in ~/.mowisai/

---

## Architecture Notes

- **Process Model:** mowisai (TUI) + agentd (socket server daemon) are independent
- **Signal Handling:** Using signal-hook crate for graceful SIGINT/SIGTERM
- **PID Tracking:** Stored in ~/.mowisai/.socket-server.pid for recovery
- **Socket Health:** Checked via Unix socket connection test + timeout
- **Daemon Spawning:** Uses sudo to start agentd with overlayfs capabilities
