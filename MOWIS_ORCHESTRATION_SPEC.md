# mowis-orchestration — Implementation Spec

> **Audience:** the coding agent that will implement this on branch
> `claude/refactor-agentd-mowis-zR6sh`.
> **Scope:** build a new `mowis-orchestration` crate, wire it into
> `mowis-host`, port the bits of `agentd` we keep, and delete the parts that
> are obsolete under the new architecture.
> **Out of scope:** changes to `mowis-executor`, `mowis-protocol` wire
> format (except small additions called out in §11), `mowis-desktop`,
> the guest VM, initrd, or vsock transport. Those are stable.

This is a long spec. Read **§1–§3** to understand the shape, then
implement strictly section-by-section in the order in §15.

---

## 1. Architecture: the four tiers

The system is a **single host process** (`mowisd`) talking to a **single
guest VM** (`mowis-executor`) over AF_VSOCK. All four LLM tiers run as
async tasks **inside the host process**. Tool calls cross vsock to the
guest. Nothing else crosses vsock.

```
                                  HOST (mowisd)
   ┌──────────────────────────────────────────────────────────────────┐
   │                                                                  │
   │   User <─── TUI / HTTP API ──── Conductor (flagship LLM)         │
   │                                    │                             │
   │                                    │  drafts Plan (multi-file)   │
   │                                    ▼                             │
   │                              .mowis/plans/<id>/                  │
   │                                    │                             │
   │           ┌────────────────────────┴────────────────────────┐    │
   │           │ in parallel                                     │    │
   │           ▼                                                 ▼    │
   │   User reviews UI                              Critic (flagship) │
   │                                                blind, one-shot   │
   │                                                no tools          │
   │                                                                  │
   │   ── Critic verdict ──> Conductor                                │
   │       (approve | revise-with-fixes)                              │
   │                                                                  │
   │   ── User approval gate ──>                                      │
   │                                                                  │
   │                              Captain (mid-tier LLM)              │
   │                                    │                             │
   │            ┌──────── spawns N ─────┼──────────────────┐          │
   │            ▼                       ▼                  ▼          │
   │       Crew #1 (fast)         Crew #2 (fast)      Crew #N (fast)  │
   │                                                                  │
   │       Each Crew = async task running a tool-calling loop         │
   │                                                                  │
   └──────────────────────────────────────────────────────────────────┘
                                       │
                                  vsock (cid:port)
                                       │
                                       ▼
   ┌──────────────────────────────────────────────────────────────────┐
   │                       GUEST VM (mowis-executor)                  │
   │  - one Level-1 sandbox per conversation                          │
   │  - one Level-2 agent-overlay per Crew                            │
   │  - tool registry (filesystem, git, docker, k8s, shell, plugins,  │
   │    skills, ...)                                                  │
   └──────────────────────────────────────────────────────────────────┘
```

### 1.1 Tier responsibilities

| Tier | Spawned | Lifetime | Model tier | Tools |
|---|---|---|---|---|
| **Conductor** | Once per conversation | Whole conversation | Flagship | Skills, plugins, repo-read (grep, read_file). **No** shell, **no** write, **no** git, **no** sandbox mutation. |
| **Critic** | Once per plan revision | One-shot | Flagship | **None.** Pure text in → structured verdict out. |
| **Captain** | After user approves the plan | Until plan completes (success, abort, or replan loop) | Mid (Sonnet-class) | Orchestration only: `create_sandbox`, `start_crew`, `get_crew_status`, `pause_crew`, `cancel_crew`, `replan`. **Does not touch user files.** |
| **Crew** | One per Plan task, spawned by Captain | Until task completes | Fast (Haiku / 4o-mini / Flash / Llama-8B) | Everything: filesystem, git, docker, k8s, http, shell, all plugins, all skills. |

### 1.2 Sandbox topology

(Decided: **One sandbox, N agent overlays.**)

- **Conversation start** → Conductor asks guest to `CreateSandbox` exactly
  once (Level-1 CoW over the project rootfs). `sandbox_id` is held by the
  Captain instance for the life of the conversation.
- **Per Crew spawn** → Captain calls a new guest endpoint
  `CreateAgentOverlay { parent_sandbox_id, agent_id }` (see §11) which
  creates a Level-2 overlay over the conversation sandbox. The Crew's
  tool calls reference `agent_id`; the executor routes them to that
  overlay.
- **On Crew success** → Captain calls `MergeAgentOverlay { agent_id }` to
  fold the agent layer into the conversation sandbox. Merges happen in
  task-dependency order; **siblings merge with a tree-merge** (log₂ N
  rounds, see `agentd/src/orchestration/merge_worker.rs` for the
  algorithm to port).
- **On Crew failure** → Captain calls `DiscardAgentOverlay { agent_id }`
  and decides (replan / retry / abort) per §6.4.

### 1.3 What does *not* change

- The `mowis-protocol` wire format is stable (only **additions** allowed,
  see §11).
- The guest (`mowis-executor`) sandbox primitives are stable. The new
  endpoints in §11 are thin wrappers around the existing sandbox module.
- The vsock transport, initrd builder, qemu launcher are unchanged.

---

## 2. Crate layout (after this change)

```
MowisAI/
├── Cargo.toml                       # workspace — add mowis-orchestration member
├── agentd/                          # SHRINKS, see §13
├── agentd-protocol/                 # unchanged (kept for runtime/ until cutover)
├── runtime/                         # unchanged for now
├── mowis-protocol/                  # +5 new payloads (§11)
├── mowis-host/                      # gains a `chat` / `serve` subcommand wired to mowis-orchestration
├── mowis-executor/                  # +5 new payload handlers (§11)
├── mowis-orchestration/             # NEW — this spec is mostly about this crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                   # public API surface (Conductor, Critic, Captain, Crew)
│       ├── conductor.rs             # §5
│       ├── critic.rs                # §6
│       ├── captain.rs               # §7
│       ├── crew.rs                  # §8 (ported from agentd/src/orchestration/agent_execution.rs)
│       ├── plan.rs                  # §4 (plan types, on-disk encode/decode)
│       ├── events.rs                # §9 (in-proc bus, event types)
│       ├── providers.rs             # §10 (port + re-export of agentd/src/orchestration/provider_client.rs)
│       ├── tools.rs                 # §12 (tier whitelists, ToolGateway client)
│       ├── config.rs                # §10 (per-tier model assignment, loaded from mowis.toml)
│       └── tests/                   # §14
└── mowis-desktop/                   # unchanged
```

Do **not** add a `mod.rs` next to `lib.rs`; use `lib.rs` only.

---

## 3. Why this shape (one paragraph each)

- **Orchestration is host-side, not guest-side.** LLM API calls have
  egress, secrets, and need observable tracing. The guest VM is the
  *target environment*, not an LLM client. Keep all `reqwest`-to-OpenAI
  / Anthropic / Vertex traffic on the host.
- **Conductor/Critic/Captain/Crew share one tokio runtime.** No
  multi-process IPC, no JSON-RPC stdin. They communicate over a
  `tokio::sync::broadcast` event bus plus per-task mpsc channels.
  Crash isolation is acceptable at the conversation level — if one
  Crew panics, we surface it to the user and let Captain decide.
- **Tools never run in the host process.** Every tool call goes over
  vsock to `mowis-executor`. This is enforced by the `ToolGateway`
  (see §12); there is no fall-through local execution.

---

## 4. `plan.rs` — the multi-file Plan format

### 4.1 On-disk layout

A Plan is a directory, not a file. Stored at `.mowis/plans/<plan_id>/`
where `<plan_id>` is `YYYYMMDDTHHMMSSZ-<6hex>` (UTC second + 6 random
hex chars).

```
.mowis/plans/<plan_id>/
├── plan.toml                # high-level metadata (see §4.2)
├── overview.md              # Conductor's prose: goal, approach, constraints
├── tasks.toml               # task graph (nodes, deps, model-tier hints) §4.3
├── sandbox.toml             # sandbox topology spec §4.4
├── models.toml              # model assignment per tier+task §4.5
├── tools.toml               # tool whitelist deltas vs tier default §4.6
├── critic/
│   ├── v1.md                # critic verdict on plan v1 (markdown)
│   ├── v1.toml              # structured verdict (approve | revise) §6.3
│   ├── v2.md
│   └── v2.toml              # only present if Conductor revised
├── history/
│   ├── v1/                  # snapshot of overview.md, tasks.toml, etc. at v1
│   ├── v2/                  # …at v2 after critic feedback
│   └── …
└── status.toml              # runtime state: which version is current,
                             # captain progress, crew statuses (§7.4)
```

The Conductor writes everything except `critic/` (Critic owns that) and
`status.toml` (Captain owns that, append-only). All files are
human-readable. No JSON in the plan dir — TOML for structured, Markdown
for prose.

### 4.2 `plan.toml` schema

```toml
plan_id      = "20260520T143155Z-a4f8c1"
created_at   = "2026-05-20T14:31:55Z"
conversation = "<conversation_id>"
current_version = 2          # which history/vN/ is live
status       = "draft"       # draft | awaiting_user | approved | running | done | aborted
user_goal    = """
multi-line verbatim user message that triggered this plan
"""
```

### 4.3 `tasks.toml` schema

```toml
[[task]]
id          = "t1"                          # short stable id
title       = "Add /healthz endpoint"
description = """
free-form, what the crew should accomplish
"""
deps        = []                            # task ids this depends on
model_tier  = "fast"                        # fast | mid | flagship (default fast)
tool_budget = 40                            # max tool calls before Captain steps in
files_hint  = ["src/server/routes.rs"]      # advisory; crew may touch others

[[task]]
id          = "t2"
title       = "Add unit test for /healthz"
deps        = ["t1"]
model_tier  = "fast"
tool_budget = 20
```

Cycles are an error. The Conductor must produce a DAG. `plan.rs`
validates this at load time.

### 4.4 `sandbox.toml` schema

```toml
# One sandbox per conversation; this just records its parameters.
image_rootfs = ".mowis-cache/rootfs/ubuntu-24.04"
ram_mb       = 8192
cpu_millis   = 4000

[overlays]
# default per-crew overlay limits (Captain may override at spawn time)
ram_mb       = 1024
cpu_millis   = 1000
```

### 4.5 `models.toml` schema

```toml
# Per-tier defaults (resolved against `tools/config.rs` provider list)
[tier.conductor]
provider = "anthropic"
model    = "claude-opus-4-7"

[tier.critic]
provider = "anthropic"
model    = "claude-opus-4-7"

[tier.captain]
provider = "anthropic"
model    = "claude-sonnet-4-6"

[tier.crew]
provider = "anthropic"
model    = "claude-haiku-4-5-20251001"

# Optional per-task override. Only the keys you want to change need be set.
[task.t1]
tier     = "mid"   # overrides default "fast" for this specific task
```

### 4.6 `tools.toml` schema

```toml
# By default each tier uses the whitelist in §12. This file applies deltas.
[crew.allow_extra]      # additional tools beyond default
plugins   = ["my-internal-deploy-script"]

[crew.deny]             # tools forbidden for this plan even though default-allowed
shell     = ["rm -rf"]  # exact-prefix denylist on arg vector
```

Deltas only — never restate the whole whitelist. The runtime composes
`tier_default ∪ allow_extra ∖ deny`.

### 4.7 `plan.rs` public API

```rust
pub struct Plan { /* parsed in-memory representation */ }

impl Plan {
    pub fn new_draft(plan_id: PlanId, user_goal: &str, conversation_id: &str) -> Self;
    pub fn load(plans_dir: &Path, plan_id: &PlanId) -> Result<Self>;
    pub fn save(&self) -> Result<()>;                  // writes the current version files
    pub fn snapshot_to_history(&mut self) -> Result<()>; // copies live files to history/vN/
    pub fn revise(&mut self, conductor_output: ConductorRevision) -> Result<()>;

    pub fn validate(&self) -> Result<(), PlanError>;   // DAG, model refs, schema
    pub fn task_graph(&self) -> &TaskGraph;
    pub fn sandbox_spec(&self) -> &SandboxConfig;
    pub fn model_for(&self, tier: Tier, task: Option<&TaskId>) -> ModelRef;
    pub fn tool_allowlist(&self, tier: Tier) -> ToolAllowlist;
}

pub struct PlanId(pub String);   // "20260520T143155Z-a4f8c1"
pub enum Tier { Conductor, Critic, Captain, Crew }

pub struct TaskGraph {
    pub tasks: Vec<TaskNode>,    // topologically sortable
}
pub struct TaskNode {
    pub id: TaskId, pub title: String, pub description: String,
    pub deps: Vec<TaskId>,
    pub model_tier: ModelTier,   // Fast | Mid | Flagship
    pub tool_budget: u32,
    pub files_hint: Vec<String>,
}
```

`Plan::save` is atomic per-file: write to `…tmp` then rename. Never
leave partial files.

---

## 5. `conductor.rs` — user-facing planner

### 5.1 Responsibilities

- Maintain conversation state (a `Vec<Message>` plus current Plan id).
- Talk to the user via the host's TUI / HTTP API.
- Decide when the user's message is a *plan request* vs *chat*.
  - Heuristic: if the user is asking a question or for an explanation,
    answer with the conductor LLM directly (no plan dir, no crew).
  - If the user is requesting work that touches files / shells / external
    APIs, draft a Plan.
- Read the repo via the **conductor's allowed tools** (skills, plugins,
  `read_file`, `list_files`, `grep`). Never write, never shell.
- Produce a fresh Plan directory (§4) and emit `PlanDrafted` on the event
  bus.
- On receiving `CriticVerdict::Revise { fixes }`, revise the Plan
  in-place, snapshot the old version to `history/vN/`, increment
  `current_version`, and re-emit `PlanDrafted`.
- On receiving `UserApproved`, hand the (now frozen) Plan to the Captain
  by emitting `PlanApproved { plan_id }`.

### 5.2 Public API

```rust
pub struct Conductor { /* owns LLM client, conversation, plan registry path */ }

impl Conductor {
    pub fn new(cfg: &OrchConfig, bus: EventBus) -> Result<Self>;
    pub async fn run(&mut self) -> Result<()>;     // main loop, drains user mpsc

    pub async fn handle_user_message(&mut self, msg: String) -> Result<ConductorAction>;
}

pub enum ConductorAction {
    Chat { reply: String },                        // no plan, just answer
    PlanDrafted { plan_id: PlanId },
    PlanRevised { plan_id: PlanId, new_version: u32 },
    AwaitingApproval { plan_id: PlanId },
}
```

### 5.3 Prompting

System prompt template lives at
`mowis-orchestration/src/prompts/conductor.md` (create the dir). Use
`include_str!` to embed. The template gets the following variables
substituted at render time (replace `{{var}}` with literal value, no
templating engine):

- `{{repo_root}}`, `{{conversation_id}}`, `{{plan_dir}}`,
  `{{available_skills_list}}`, `{{available_plugins_list}}`,
  `{{provider_capabilities}}` (a short note on which providers can
  handle long context, etc.).

Tool schema set for the LLM: see §12.1.

### 5.4 Errors that surface to the user

- `PlanError::Cycle` — conductor produced an invalid DAG. Conductor
  must self-correct (re-prompt with the validation message) before
  emitting `PlanDrafted`. Max 3 self-correction attempts; then
  surface to the user.
- LLM provider error → render to user as a one-line summary + show
  the conversation can continue.

---

## 6. `critic.rs` — blind one-shot reviewer

### 6.1 Trigger

The Critic listens for `PlanDrafted` events. On each event it spawns
**one** LLM call. Multiple plan versions in a conversation = multiple
Critic invocations (one per version). The Critic instance is stateless;
each invocation reads the Plan directory fresh.

### 6.2 Inputs

- The Plan directory's `overview.md`, `tasks.toml`, `sandbox.toml`,
  `models.toml`, `tools.toml`. **Not** the conversation history. **Not**
  the user's raw message (deliberate: blind context). The Critic reviews
  the Plan *as a Plan*, not the user's framing of it.
- A fixed rubric (system prompt) that includes:
  1. Correctness — do the tasks actually accomplish the stated overview?
  2. DAG hygiene — are dependencies right? any obviously missing tasks?
  3. Safety — anything in the plan that touches production / external
     side-effects without explicit user goal?
  4. Tool/model fit — are crew tasks budgeted reasonably? are flagship
     models used where they shouldn't be?
  5. Sandbox topology — overlays sized appropriately for tasks?

### 6.3 Output: structured verdict

The Critic writes two files to `critic/`:

- `vN.md` — full prose review (what the user sees).
- `vN.toml` — machine-parseable verdict:

```toml
verdict     = "approve"           # approve | revise | block
summary     = "Plan is sound; one minor concern on task ordering."
[[issue]]
severity    = "info"              # info | warn | block
section     = "tasks.toml"
message     = "t3 depends on t2 but t3 doesn't actually need t2's output."
suggested_fix = "Remove t3.deps = [\"t2\"]."

[[issue]]
severity    = "warn"
section     = "models.toml"
message     = "..."
```

Mapping verdict → next step:
- `approve` → Critic emits `CriticVerdict::Approve { plan_id, version }`.
  User now needs to click Approve in UI; Captain spawns on that click.
- `revise` → Critic emits `CriticVerdict::Revise { plan_id, version, fixes }`.
  Conductor wakes up, revises the plan (new version), re-emits
  `PlanDrafted`. The user sees both versions and the Critic's notes side
  by side. **Critic re-runs on the new version.**
- `block` → Critic emits `CriticVerdict::Block { plan_id, version, reason }`.
  User UI shows the block prominently. User can either accept the block
  (cancel the plan) or explicitly override (emits `UserOverride` which
  bypasses Critic and routes straight to approval gate).

### 6.4 Loop guard

Hard cap: 4 plan versions per conversation turn. If the Critic still
says `revise` on v4, escalate to the user: "Critic and I disagree;
here are both views, you decide."

### 6.5 Public API

```rust
pub struct Critic { /* LLM client only */ }

impl Critic {
    pub fn new(cfg: &OrchConfig, bus: EventBus) -> Result<Self>;
    pub async fn run(&mut self) -> Result<()>;     // subscribes to PlanDrafted
    pub async fn review_once(&self, plan: &Plan, version: u32) -> Result<Verdict>;
}
```

System prompt at `mowis-orchestration/src/prompts/critic.md`.

---

## 7. `captain.rs` — execution orchestrator

### 7.1 Trigger

Spawned only after the event bus emits `PlanApproved { plan_id }`. The
host wires this up: on `PlanApproved`, instantiate one `Captain` and
call `.run()` on it.

### 7.2 Responsibilities

1. Load the (now frozen) Plan via `Plan::load`.
2. Resolve the sandbox: call `CreateSandbox` on the executor with the
   plan's `sandbox.toml`. Store `sandbox_id` in `status.toml`.
3. Compute task execution order from `tasks.toml` (topological sort,
   then schedule in parallel waves where dependencies allow).
4. For each ready task:
   - Call `CreateAgentOverlay { parent_sandbox_id, agent_id }` (§11).
   - Spawn a `Crew` (§8) on a tokio task with the task spec, agent_id,
     and the model resolved from `models.toml`.
   - Subscribe to that crew's events on the bus.
5. As Crew tasks complete:
   - `Done` → merge their overlay (§7.5).
   - `Failed` → consult retry policy (§7.6).
6. When all tasks complete OR an unrecoverable failure happens, write
   final status to `status.toml`, emit `PlanCompleted` /
   `PlanFailed`, and shut down.

The Captain is **also an LLM loop** — it has a Sonnet-tier model and
the tools listed in §12.4. The LLM is consulted when the Captain needs
to make a judgement call: should we retry this Crew? was the merge
clean enough? should we ask the Conductor to replan a subgraph? The
deterministic scheduling above is the default; the LLM is the
fallback / escalation path.

### 7.3 Tools the Captain can call

See §12.4. Tight whitelist: `create_sandbox`, `create_agent_overlay`,
`start_crew`, `get_crew_status`, `pause_crew`, `cancel_crew`,
`merge_overlay`, `discard_overlay`, `replan_subgraph`. No filesystem
read/write, no shell, no git. These are **synthetic** tools that
dispatch to internal functions, not vsock calls (except the overlay /
sandbox ones which do go to the executor).

### 7.4 `status.toml` schema (append-only)

```toml
plan_id          = "20260520T143155Z-a4f8c1"
captain_started  = "2026-05-20T14:35:01Z"
sandbox_id       = "sb-7f1a"

[[event]]
ts    = "2026-05-20T14:35:02Z"
kind  = "crew_started"
task  = "t1"
agent = "ag-001"

[[event]]
ts    = "2026-05-20T14:35:18Z"
kind  = "crew_done"
task  = "t1"
agent = "ag-001"
tool_calls = 12
```

The Captain only appends. The host can tail this file for live UI
updates.

### 7.5 Merge policy

- Sibling tasks (no deps between them) merge in dependency-tree order
  using `merge_worker.rs`'s tree-merge (port that module — see §13.2).
- Two-way merge first (overlay vs current sandbox state). On conflict,
  the Captain LLM is consulted with both file versions and the task
  descriptions, and produces a 3-way resolution. Cap conflict-repair
  attempts at 3.
- After all merges, the sandbox represents the final desired state.

### 7.6 Retry policy

Per task:
- Tool failure inside a Crew → that Crew retries locally up to 3 times
  (already in `agent_execution.rs`).
- Crew exits with `Failed` → Captain retries up to **2 times** with the
  same agent overlay (discarding previous). On 3rd failure, Captain LLM
  decides: `replan_subgraph` (asks Conductor to redraft just this task
  and its downstream) or `abort`.

### 7.7 Public API

```rust
pub struct Captain { /* sandbox handle, plan, LLM client, bus, crew handles */ }

impl Captain {
    pub fn new(cfg: &OrchConfig, plan_id: PlanId, bus: EventBus) -> Result<Self>;
    pub async fn run(mut self) -> Result<CaptainOutcome>;
}

pub enum CaptainOutcome {
    Completed { sandbox_id: String },
    Failed { reason: String, sandbox_id: String, /* sandbox kept for inspection */ },
    Aborted,
}
```

System prompt at `mowis-orchestration/src/prompts/captain.md`.

---

## 8. `crew.rs` — per-task LLM loop

### 8.1 Port from existing code

This is the closest thing to a straight port. Source:
`agentd/src/orchestration/agent_execution.rs` (645 lines). What to keep:

- `AgentExecutor` struct & `execute_task` method shape.
- The tool-calling round loop using `provider_client::call_agent_round`.
- Checkpoint after every tool call (port `agentd/src/orchestration/checkpoint.rs`).

What to change:

- Tool dispatch: **must** go through the `ToolGateway` (§12.2) which
  routes to vsock instead of running tools in-process.
- The sandbox id ↔ agent id ↔ overlay id model is different. The Crew
  receives `(sandbox_id, agent_id)` and passes both on every tool call.
- Drop the old `cost_tracker` / `verification` couplings — Captain owns
  those decisions now.
- Drop `agent_templates.rs` couplings — Crew is plain LLM + tools, the
  system prompt is a template at
  `mowis-orchestration/src/prompts/crew.md` parameterised with the
  task's title/description/files_hint.

### 8.2 Loop

```
loop {
    round = call_agent_round(model, conversation, tool_schemas).await?;
    if round.is_final() { break }
    for tool_call in round.tool_calls {
        if !whitelist.allows(tool_call.name) {
            conversation.push_tool_results(vec![(tool_call, json!({"error": "tool forbidden"}))]);
            continue;
        }
        result = tool_gateway.invoke(sandbox_id, agent_id, tool_call).await;
        checkpoint::save(&conversation);
        conversation.push_tool_results(vec![(tool_call, result)]);
    }
    if conversation.rounds >= task.tool_budget { break with budget_exceeded }
}
emit CrewEvent::Done { agent_id, summary }   // or Failed
```

### 8.3 Public API

```rust
pub struct Crew { /* LLM cfg, task, ids, tool_gateway, bus */ }

impl Crew {
    pub fn new(...) -> Self;
    pub async fn run(self) -> Result<CrewOutcome>;
}

pub enum CrewOutcome {
    Done { agent_id: String, summary: String, tool_calls: u32 },
    Failed { agent_id: String, reason: String, tool_calls: u32 },
}
```

### 8.4 Concurrency

Crews run as independent tokio tasks. There is no global lock around
their tool calls — the vsock transport multiplexes by request id (see
`mowis-host/src/transport/mod.rs`). The executor handles concurrency on
its side (one connection per host transport instance is fine for MVP;
revisit if it becomes a bottleneck).

---

## 9. `events.rs` — in-proc event bus

### 9.1 Transport

`tokio::sync::broadcast::Sender<Event>` cloned into each tier. Capacity
1024. Lagged receivers log a warning and continue (events are
informational, not critical state — `status.toml` is the durable
record).

### 9.2 Event enum

```rust
#[derive(Debug, Clone)]
pub enum Event {
    // From Conductor
    PlanDrafted   { plan_id: PlanId, version: u32 },
    PlanRevised   { plan_id: PlanId, version: u32 },
    PlanApproved  { plan_id: PlanId },

    // From Critic
    CriticReviewing { plan_id: PlanId, version: u32 },
    CriticVerdict { plan_id: PlanId, version: u32, verdict: Verdict },

    // From user
    UserApproved     { plan_id: PlanId },
    UserOverride     { plan_id: PlanId },   // bypass critic block
    UserCancelled    { plan_id: PlanId },

    // From Captain
    CaptainStarted   { plan_id: PlanId, sandbox_id: String },
    CrewStarted      { plan_id: PlanId, task_id: TaskId, agent_id: String },
    CrewProgress     { plan_id: PlanId, agent_id: String, tool: String, round: u32 },
    CrewDone         { plan_id: PlanId, agent_id: String, summary: String },
    CrewFailed       { plan_id: PlanId, agent_id: String, reason: String },
    MergeStarted     { plan_id: PlanId, agent_id: String },
    MergeCompleted   { plan_id: PlanId, agent_id: String },
    PlanCompleted    { plan_id: PlanId },
    PlanFailed       { plan_id: PlanId, reason: String },
}

#[derive(Debug, Clone)]
pub enum Verdict {
    Approve,
    Revise { issues: Vec<Issue> },
    Block  { reason: String, issues: Vec<Issue> },
}
```

### 9.3 Bus wiring

```rust
pub struct EventBus(broadcast::Sender<Event>);

impl EventBus {
    pub fn new() -> Self;
    pub fn sender(&self) -> broadcast::Sender<Event>;
    pub fn subscribe(&self) -> broadcast::Receiver<Event>;
    pub fn emit(&self, ev: Event);  // logs + sends
}
```

Every tier holds an `EventBus`. The host process (mowisd's `chat`
subcommand) constructs one bus and clones it into each tier.

---

## 10. `providers.rs` + `config.rs` — model wiring

### 10.1 Reuse existing provider client

**Do not re-implement provider integrations.** Port `LlmConfig`,
`generate_text`, `generate_chat`, and `call_agent_round` verbatim from
`agentd/src/orchestration/provider_client.rs` (1911 lines) into
`mowis-orchestration/src/providers.rs`. The only changes:

- Replace `crate::config::AiProvider` (agentd) with a local `Provider`
  enum copied from `agentd/src/config.rs:5-17` (variants: `VertexAi`,
  `Grok`, `Groq`, `Anthropic`, `OpenAi`, `Gemini`, `Mimo`).
- Replace `super::streaming::{StreamEvent, StreamWriter}` with locally
  defined types in `providers.rs` (port from
  `agentd/src/orchestration/streaming.rs` — keep the surface, drop
  unused features).
- Replace the dependency on `crate::config::MowisConfig` with the new
  `OrchConfig` (defined below).
- Re-export the same public functions: `generate_text`,
  `generate_text_with_limit`, `generate_chat`, `call_agent_round`,
  `call_agent_round_streaming`.

### 10.2 New config file

Path: `~/.mowisai/mowis.toml` (separate from the existing
`~/.mowisai/config.toml` agentd uses — do not migrate or clobber it).

```toml
[providers.anthropic]
api_key_enc = "…AES-256-GCM blob, same encryption as agentd/src/crypto.rs…"

[providers.openai]
api_key_enc = "…"

[providers.gemini]
api_key_enc = "…"

[providers.vertex_ai]
project_id  = "company-internal-tools-490516"
# no api_key — uses gcloud ADC

[providers.grok]
api_key_enc = "…"

[providers.groq]
api_key_enc = "…"

[providers.mimo]
api_key_enc = "…"

[tier.conductor]
provider = "anthropic"
model    = "claude-opus-4-7"

[tier.critic]
provider = "anthropic"
model    = "claude-opus-4-7"

[tier.captain]
provider = "anthropic"
model    = "claude-sonnet-4-6"

[tier.crew]
provider = "anthropic"
model    = "claude-haiku-4-5-20251001"

[sandbox]
image_rootfs = ".mowis-cache/rootfs/ubuntu-24.04"
ram_mb       = 8192
cpu_millis   = 4000
```

Per-plan overrides (in `.mowis/plans/<id>/models.toml`) compose on top.

### 10.3 Public API

```rust
pub struct OrchConfig {
    pub providers: HashMap<Provider, ProviderCreds>,
    pub tiers:     HashMap<Tier, ModelRef>,
    pub sandbox:   SandboxConfig,
    pub plans_dir: PathBuf,           // default `.mowis/plans/`
}

impl OrchConfig {
    pub fn load() -> Result<Self>;           // reads ~/.mowisai/mowis.toml
    pub fn save(&self) -> Result<()>;
    pub fn llm_for(&self, tier: Tier) -> Result<LlmConfig>;
    pub fn llm_for_task(&self, plan: &Plan, task: &TaskNode) -> Result<LlmConfig>;
}

pub struct ModelRef {
    pub provider: Provider,
    pub model:    String,
}
```

### 10.4 Encryption

Use the **exact same AES-256-GCM scheme as agentd**. Port
`agentd/src/crypto.rs` (don't add a new dep). The master key lives at
`~/.mowisai/.key` with mode `0o600`.

---

## 11. `mowis-protocol` additions (the only wire change)

Add to `mowis-protocol/src/lib.rs` `enum Payload`:

```rust
// Requests (host -> guest)
CreateAgentOverlay  { parent_sandbox_id: String, agent_id: String,
                      limits: ResourceLimits },
MergeAgentOverlay   { parent_sandbox_id: String, agent_id: String },
DiscardAgentOverlay { parent_sandbox_id: String, agent_id: String },
InvokeToolAsAgent   { parent_sandbox_id: String, agent_id: String,
                      tool: String, input: serde_json::Value },

// Responses (guest -> host)
AgentOverlayCreated   { agent_id: String },
AgentOverlayMerged    { agent_id: String, changed_paths: Vec<String> },
AgentOverlayDiscarded { agent_id: String },
```

Add roundtrip tests in `mowis-protocol/src/lib.rs` `mod tests`.
Bump `PROTOCOL_VERSION` from 1 to 2.

### 11.1 Executor implementation

In `mowis-executor/src/server.rs`:

- Extend `dispatch` (line 71) to match the new payloads.
- Add `async fn create_agent_overlay`,
  `async fn merge_agent_overlay`,
  `async fn discard_agent_overlay`,
  `async fn invoke_tool_as_agent` next to the existing handlers.
- Sandbox primitives in `mowis-executor/src/sandbox.rs` need a thin
  helper for the agent-overlay layer. The existing CoW infrastructure
  used for `create_sandbox` already supports nested overlays — wrap it.

### 11.2 Tool dispatch on the guest

The existing `mowis-executor/src/tools.rs` (79 lines) is a stub. It
must be expanded to a full registry analogous to
`agentd/src/tools/mod.rs` (165 `create_*_tool` factories) but **the tool
implementations must execute against the agent's overlay**, not the
host. Port the tool implementations from `agentd/src/tools/*.rs` into
`mowis-executor/src/tools/`. This is mechanical — the
`filesystem.rs`, `git.rs`, `shell.rs`, etc. files already operate via
`ToolContext` paths, which become guest-local paths after the port.

**Scope note:** porting all 165 tools is a big job. Phase it (§15.5):
phase 1 ports filesystem + shell + http + git only; phases 2+
add the rest.

---

## 12. `tools.rs` — tier whitelists & ToolGateway

### 12.1 Conductor tool whitelist

| Tool | Notes |
|---|---|
| `read_file` | local repo only, no `/etc` or `/proc` |
| `list_files` | local repo only |
| `grep` / `find` | local repo only |
| `skill.*` | any user-defined skill |
| `plugin.*` | any installed plugin |

Forbidden: shell, write_file, git, http (network), docker, k8s, anything
that mutates the workspace or the world.

### 12.2 ToolGateway

The Crew (and Conductor, for read-only tools) never call the executor
RPC directly. They go through `ToolGateway`:

```rust
pub struct ToolGateway {
    transport: Arc<Mutex<VsockTransport>>,   // re-uses mowis-host/src/transport
    tier:      Tier,
    allowlist: ToolAllowlist,
}

impl ToolGateway {
    pub async fn invoke(&self,
                        sandbox_id: &str,
                        agent_id:   &str,         // for crew; empty for conductor
                        call:       ToolCall) -> Result<serde_json::Value>;
}
```

The gateway:
1. Checks `self.allowlist.allows(&call.name)` → if not, returns an
   `error` Value (the LLM sees it as a tool error, not a panic).
2. Translates the `ToolCall` into the right protocol payload
   (`InvokeToolAsAgent` for Crew, `InvokeTool` for Conductor's
   read-only set if/when we route those — for Conductor we can also
   short-circuit and read files directly from the host since the repo
   is on the host).
3. Awaits the response with a per-tier timeout (Crew: 5 min default;
   Conductor: 30s).

### 12.3 Critic — no gateway

The Critic has no `ToolGateway`. Its constructor doesn't even take one.
Build-time guarantee.

### 12.4 Captain tool whitelist

Captain's tools are **internal**, not LLM-arbitrary. The Captain's LLM
sees a small synthetic schema:

```
- start_crew(task_id)
- get_crew_status(agent_id)
- pause_crew(agent_id)
- cancel_crew(agent_id)
- merge_overlay(agent_id)
- discard_overlay(agent_id)
- replan_subgraph(root_task_id, reason)
```

Each of those dispatches to a method on `Captain`, not over vsock
(except `merge_overlay`/`discard_overlay` which do go over vsock as
described in §11). The Captain LLM cannot file-edit, shell, or call
plugins.

### 12.5 Crew tool whitelist

Default-allow everything. Apply deltas from `tools.toml` (§4.6).
Server-side enforcement: every `InvokeToolAsAgent` carries the caller's
tier in a header (add a `caller_tier: String` field on
`InvokeToolAsAgent` in §11). The executor rejects forbidden combinations
even if the host's gateway is buggy.

---

## 13. `agentd` shrinkage

### 13.1 Delete

After mowis-orchestration is wired up and the new `mowisd chat` works
end-to-end, **delete**:

- `agentd/src/intent.rs`
- `agentd/src/orchestration/planning_modes.rs`
- `agentd/src/orchestration/complexity_classifier.rs`
- The pipeline-picker switch logic in
  `agentd/src/orchestration/new_orchestrator.rs` (the function that
  picks between "tiny / small / medium / large" plans based on the
  complexity classifier — find it via grep). Keep the rest of
  `new_orchestrator.rs` for now; it can die with the agentd cutover.

Do **not** delete in this PR:
- `agentd/src/socket_server.rs` (still needed by `runtime/` and HTTP API)
- `agentd/src/tui/*` (separate cutover)
- `agentd/src/api_server.rs` (separate cutover)
- The `agentd-protocol` crate (used by runtime)

### 13.2 Port (don't copy — move, then re-export)

| Source | Destination | Notes |
|---|---|---|
| `agentd/src/orchestration/provider_client.rs` | `mowis-orchestration/src/providers.rs` | §10.1 |
| `agentd/src/orchestration/streaming.rs` | `mowis-orchestration/src/providers.rs` (inline) | drop unused bits |
| `agentd/src/orchestration/agent_execution.rs` | `mowis-orchestration/src/crew.rs` | §8.1 |
| `agentd/src/orchestration/checkpoint.rs` | `mowis-orchestration/src/crew.rs` (inline) | crew-internal |
| `agentd/src/orchestration/merge_worker.rs` (algorithm only — file diff/merge, **not** the agentd worker spawning) | `mowis-orchestration/src/captain.rs` (private `mod merge` inside) | §7.5 |
| `agentd/src/crypto.rs` | `mowis-orchestration/src/crypto.rs` | §10.4 |
| `agentd/src/tools/*.rs` (filesystem, shell, http, git, etc.) | `mowis-executor/src/tools/*.rs` | §11.2 — phased |

After porting, leave a `pub use mowis_orchestration::providers::*;`
re-export inside agentd for any code that still calls it during
cutover. Delete those re-exports at agentd cutover time.

### 13.3 Keep, untouched

`agentd/src/{config.rs, setup.rs, persistence.rs, tui/*, api_server.rs,
socket_server.rs, sandbox.rs, vm_backend.rs, guest_backend.rs,
worker_agent.rs, hub_agent.rs, agent_loop.rs, dependency_graph.rs,
memory.rs, audit.rs, security.rs, channels.rs, logging.rs, image_manager.rs,
buckets.rs, version.rs, lib.rs, main.rs, tool_registry.rs, protocol.rs,
{anthropic,openai,gemini,grok,groq,vertex}_agent.rs}`. These all stay
exactly as they are.

---

## 14. Testing

### 14.1 Unit tests

- `plan.rs`: TOML round-trip for every file in the plan dir; DAG cycle
  detection; revise+history snapshotting; load+save idempotence.
- `providers.rs`: keep the existing tests from `provider_client.rs`.
  Add a fake-provider impl used only in tests.
- `events.rs`: bus delivers to multiple subscribers; lagged subscriber
  doesn't deadlock the sender.
- `tools.rs`: `ToolAllowlist::allows` semantics; deltas applied
  correctly; Critic constructor refuses to accept a gateway (compile
  test).

### 14.2 Integration tests (no LLM, no VM)

Add `mowis-orchestration/tests/integration_fake_llm.rs`:

- Stub LLM provider that returns scripted responses.
- Stub vsock transport that records and acks every payload.
- End-to-end: Conductor → Plan → Critic (approve) → user_approve →
  Captain → 2 Crews → merge → done. Assert event sequence on the bus
  and final `status.toml`.

### 14.3 End-to-end smoke (with real VM, optional gate)

Extend `scripts/full-test.sh` (already exists) with a Phase F:

- `mowisd chat --script tests/scripts/healthz.txt`
  — script feeds the user side (one canned prompt + auto-approval).
- Assert the conversation sandbox contains the expected file changes
  after `PlanCompleted`.

Skip in CI by default (real LLM cost) — opt in via
`MOWIS_E2E_LLM=1`.

---

## 15. Implementation order (do these in this order, do not skip ahead)

### 15.1 Crate scaffolding (no logic)

- Add `mowis-orchestration` to root `Cargo.toml` workspace members.
- Create `mowis-orchestration/Cargo.toml` with deps: `tokio` (full),
  `serde`, `serde_json`, `toml`, `anyhow`, `thiserror`, `tracing`,
  `reqwest`, `once_cell`, `mowis-protocol`, `mowis-host` (path dep,
  for the transport), and the existing AES/sha crates from
  `agentd/Cargo.toml`.
- Create `mowis-orchestration/src/lib.rs` with empty `pub mod`
  declarations for every module in §2.
- `cargo check` must pass.

### 15.2 Protocol additions

- Implement §11 in `mowis-protocol/src/lib.rs` (additions only).
- Add roundtrip tests for each new payload.
- Implement §11.1 handlers in `mowis-executor/src/server.rs` and
  `mowis-executor/src/sandbox.rs`. Tools dispatch can be a stub for
  now (return `Error { message: "not implemented" }`).
- `cargo test -p mowis-protocol -p mowis-executor` must pass.

### 15.3 Providers + Config

- Port `provider_client.rs` and `streaming.rs` from agentd into
  `mowis-orchestration/src/providers.rs` per §10.1.
- Port `agentd/src/crypto.rs` into
  `mowis-orchestration/src/crypto.rs`.
- Implement `OrchConfig` per §10.3.
- Add a CLI subcommand `mowisd config init` that creates
  `~/.mowisai/mowis.toml` from scratch (interactive, prompts for
  providers, encrypts keys).
- Unit tests: round-trip mowis.toml; LlmConfig resolution for each
  tier.

### 15.4 Plan + Events

- Implement `plan.rs` per §4.
- Implement `events.rs` per §9.
- Tests per §14.1.

### 15.5 Tools registry on the guest (phase 1: minimal)

- Create `mowis-executor/src/tools/` with `filesystem.rs`, `shell.rs`,
  `http.rs`, `git.rs`, `mod.rs`. Port from `agentd/src/tools/`.
- Wire `InvokeToolAsAgent` (§11.1) to dispatch to this registry.
- A tool call from the host with `tool="read_file"` should round-trip
  successfully.

### 15.6 ToolGateway

- Implement `tools.rs` (host side) per §12.
- Wire it on top of `mowis-host/src/transport/`. (Look at how
  `mowisd ping` and `mowisd exec` use the transport for the pattern.)

### 15.7 Crew

- Port `agent_execution.rs` + `checkpoint.rs` into `crew.rs` per §8.
- Replace in-process tool execution with `ToolGateway::invoke`.
- Stub the prompts dir with a minimal `crew.md`.
- Integration test: stub LLM produces a single `read_file` call;
  Crew invokes via gateway; gateway round-trips to executor; result
  flows back; Crew emits `Done` on the bus.

### 15.8 Captain

- Implement `captain.rs` per §7.
- The Captain LLM is initially a no-op: deterministic scheduling
  only, no LLM call. Wire the LLM call after the deterministic path
  works end-to-end.
- Port `merge_worker.rs` algorithm; integrate with `MergeAgentOverlay`.
- Integration test: 2-task plan, both succeed, both merge, captain
  emits `PlanCompleted`.

### 15.9 Critic

- Implement `critic.rs` per §6.
- System prompt `prompts/critic.md` with the rubric in §6.2.
- Integration test (stub LLM): plan v1 → critic says `revise` →
  conductor revises → plan v2 → critic says `approve` → user
  approves → captain spawns.

### 15.10 Conductor

- Implement `conductor.rs` per §5.
- Wire to host CLI: new subcommand `mowisd chat` that starts a
  conversation loop on stdin (TUI integration in a later cutover).
- System prompt `prompts/conductor.md`.

### 15.11 Host wiring

- Add `Cmd::Chat` to `mowis-host/src/main.rs`'s subcommand enum.
- `mowisd chat` constructs: `OrchConfig::load()`, `EventBus::new()`,
  the four tiers, then drives `Conductor::run()`. The other tiers
  subscribe on the bus.
- A pass-through stdin/stdout REPL is enough for v1.

### 15.12 agentd shrinkage

- Delete the files in §13.1.
- Migrate any callers (likely zero outside `new_orchestrator.rs`) to
  the new crate.
- `cargo build --workspace` must pass.

### 15.13 Tests + E2E

- Run §14.1, §14.2 in CI.
- Run §14.3 manually with a small Plan against a real provider.

### 15.14 BUILD_NUMBER bump

Bump `agentd/src/version.rs` `BUILD_NUMBER` before any push to main
(per CLAUDE.md). Format: `YYYYMMDD.N`.

---

## 16. Hard rules (do not violate)

1. **No tool runs in the host process.** Every tool — even Conductor's
   read_file — dispatches to `mowis-executor` over vsock. The only
   exception: the Captain's synthetic orchestration tools (§12.4),
   which are method calls on Captain itself.
2. **Critic is stateless and blind.** It never sees conversation
   history, only the Plan dir. Its constructor cannot accept a
   `ToolGateway` or any reference to user messages.
3. **The Plan is the source of truth.** The bus is informational; if
   the host crashes mid-execution, recovery reads `.mowis/plans/<id>/`
   and `status.toml`.
4. **No `unwrap()` in production code paths.** Use `?` or proper
   error handling (CLAUDE.md invariant).
5. **No direct crew-to-crew communication.** Crews only see their own
   overlay. Cross-task data flow happens by Captain merging overlays
   in dependency order.
6. **`mowis-protocol` wire version bumps to 2** in §15.2. Both host
   and executor refuse to talk to a peer with a different
   PROTOCOL_VERSION (already implemented for v1; just bump the
   constant).
7. **No bypass of the tool whitelist.** Both bind-time and server-side
   enforcement must agree. If they disagree, that's a bug.
8. **All TOML in plan dirs is atomic** (write tmp + rename).
9. **Tests must pass before each section is considered done.**
10. **Never delete or modify tests to make them pass** (CLAUDE.md).

---

## 17. Open questions to ask the user before starting a section

(All listed up front so the coding agent can batch.)

- §4.3 task graph: should we support task-level retries declared in
  the plan, or is that purely Captain's runtime call? *Default
  assumption: runtime call only; no `retry =` key in tasks.toml.*
- §6.3 critic `block` verdict: should it surface a "fix it for me"
  button that just runs the Conductor revise loop, or is "block"
  always terminal unless user overrides? *Default assumption:
  terminal unless overridden.*
- §10.2 mowis.toml location: ok with `~/.mowisai/mowis.toml` separate
  from existing `~/.mowisai/config.toml`? *Default assumption: yes,
  two files, no migration.*
- §15.5 tools registry phase 1: filesystem + shell + http + git
  enough to demo end-to-end, or do we need docker too? *Default
  assumption: those four are enough; docker/k8s in phase 2.*
- §15.11 host wiring: TUI integration is out of scope for this PR.
  Stdin REPL only. Confirm? *Default assumption: yes.*

The coding agent should answer each with the default assumption unless
the user has said otherwise in the same conversation.

---

## 18. Done means

- `cargo build --workspace` passes on the branch.
- `cargo test --workspace` passes.
- `mowisd chat` starts, accepts a stdin prompt, drafts a plan, runs
  the critic, lets the user approve via a stdin `y`, spawns the
  captain, runs at least one crew with a real tool call against the
  guest VM, merges the overlay, and prints `PlanCompleted`.
- `.mowis/plans/<id>/` contains the expected files (§4.1).
- All §13.1 deletions are done.
- BUILD_NUMBER bumped (§15.14).
- No `unwrap()`, no `.expect()` in non-test code.

When all those are true, the coding agent opens a PR. I will review
the diff against this spec section by section before any merge.
