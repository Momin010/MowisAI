# Agentd – MowisAI Sandbox Runtime

`agentd` is the core runtime engine for the MowisAI sandboxed agent platform.  It
provides an isolated, resource‑limited environment where tools are registered
and executed in response to prompts or programmatic requests.  The library can
be used directly from Rust code, exposed via a small C FFI, or driven via a
lightweight Unix socket API (see `src/socket_server.rs`).


TO RUN THE ENGINE AND START THE SOCKET. 
cd agentd
cargo build --release
pkill agentd 2>/dev/null
sudo ./target/release/agentd socket --path /tmp/agentd.sock


---

## 🚀 Key Concepts

- **Sandbox** – a top-level isolated environment with RAM/CPU limits, an
  optional root filesystem, a catalog of registered tools, and a security
  policy.
- **Container** – lightweight overlayfs layer created from a sandbox; all tool
  invocations happen inside the container, never on the host OS.
- **Tool** – pluggable operation (e.g. `read_file`, `http_get`, `git_clone`) that
  implements the `Tool` trait.  Tools execute under the sandbox's restrictions.
- **SecurityPolicy** – rules governing file and network access; evaluates
  requests prior to execution and rejects disallowed actions.
- **AgentLoop** – high‑level planning/execution loop which selects tools based on
  memory and prompt context. Useful for building recursive agents.
- **FFI / CLI** – minimal C bindings for embedding; the `agentd` binary provides
  a lightweight CLI and a socket server for remote control.

---

## 🛠️ Building the Project

```sh
cd agentd
cargo build          # normal build
cargo test           # run unit & integration tests
cargo test -- --ignored   # exercise "heavy" tests that may require root
```

> **Note:** A number of tests are marked `#[ignore]` because they require
> running as root or depend on external services.  The recent `engine_tests.rs`
> file has un‑ignored tests demonstrating the current engine features.

---

## 💡 Usage Examples

### Using the Rust Library

```rust
use libagent::{Sandbox, ResourceLimits, SecurityPolicy, AgentLoop};
use serde_json::json;

let mut sb = Sandbox::new(ResourceLimits { ram_bytes: None, cpu_millis: None })?;
sb.set_policy(SecurityPolicy::default_permissive());
sb.register_tool(libagent::tools::create_echo_tool());
let cid = sb.create_container()?;
let res = sb.invoke_tool_in_container(cid, "echo", json!({"message":"hi"}))?;
println!("got {:?}", res);

// agent loop
let mut loop_engine = AgentLoop::new(1, 1, 10);
let out = loop_engine.run("repeat me", &vec![libagent::tools::create_echo_tool()])?;
println!("agent result {:?}", out);
```

### Command‑Line Interface

The binary exposes a very light CLI *primarily for development/testing*:

```sh
agentd create-sandbox --ram 100000000 --cpu 1000
agentd socket --path /tmp/agentd.sock   # start API server
# use the JS `test-*.js` clients or send JSON to the socket directly
```

Most commands are stubs pointing at the library API; the recommended path is
to use the Rust crate directly or the socket protocol shown in the `tests/`
JavaScript clients.

---

## 🧪 Testing

Tests are located under `agentd/tests`.  Early versions of the repo included
extensive, human‑readable integration tests that assumed an older engine; those
have been left for reference but new code should live in `engine_tests.rs`.

The modern test suite exercises the current API:

* `engine_tests.rs` – container invocation, security policies, simple agent loop
* `sandbox_tests.rs` – ID sequencing, child sandbox limits, cgroup checks
* `filesystem_tools_tests.rs`, `shell_tools_tests.rs`, etc. – per‑tool unit tests

To re‑run all tests (including the ignored ones):

```sh
cd agentd
git clean -fdx        # ensure a clean state
cargo test -- --ignored
```

When adding new features, update or extend the tests here.  Avoid weld‑in
logic that depends on the old `invoke_tool` API; use the container helpers or
`AgentLoop` directly.

---

## ⚠️ Known Issues & TODOs

* **Numerous compiler warnings.**  Several unused imports and fields remain;
a `cargo fix` pass would eliminate them.  Static analysis currently reports the
`config` field in `Agent` as unused, etc.
* **Incomplete security policy coverage.**  Only a few tool types are checked
explicitly.  Future work should generalize the policy framework.
* **Resource cleanup.**  `Sandbox::drop` attempts to unmount but does not always
remove overlay directories on failure.
* **FFI memory handling.**  C helpers now correctly drop returned objects, but
clients must still call `agent_string_free`/`agent_sandbox_free`.
* **Tests are noisy.**  Many integration tests require `--ignored` to run, and
Rust tests produce a cascade of warnings; consider slimming or disabling
development‑only modules.

Contributions are welcome – see `CONTRIBUTING.md` (not yet present) for
coding style guidelines.

---

## 🗂️ Tool Inventory

The runtime registers **75 tools** by default spanning filesystem operations,
shell commands, HTTP, Git, Docker, Kubernetes, storage/memory, package
managers, web search, channels/messaging, and various utilities.  The full
list is defined in `src/tool_registry.rs` and helper factories live in
`src/tools/mod.rs`.

To add a custom tool, implement the `Tool` trait, expose a factory, and
register it with `sandbox.register_tool`.  The socket server's `register_tool`
endpoint can load any tool by name using the global registry.

---

## 📦 Packaging & FFI

The crate exposes a minimal C API (`agent_sandbox_new`, `agent_sandbox_run`,
`agent_string_free`, etc.) so it can be embedded in other languages.  The
`examples/complete_usage.rs` file demonstrates building a small application
that drives an agent via the Rust API.

---

## 🔍 Inspection & Debugging

Log output is controlled via `env_logger`.  At runtime you can set
`RUST_LOG=info` or `debug` to see sandbox creation, tool registration, and
audit events.

Audit events are captured by `src/audit.rs` and written to the global
`AUDITOR` object; they indicate sandbox creation, tool invocation, policy
violations, etc.

---

Thanks for exploring `agentd`!  If you need help understanding a particular
module, the source is intentionally small and well‑commented – start with
`src/sandbox.rs` and `src/tools/mod.rs`.
