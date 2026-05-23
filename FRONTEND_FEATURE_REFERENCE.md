# MowisAI — Complete Feature Reference for Frontend

## AGENTS (4 tiers)

### 1. Conductor (user-facing planner)
- **Lifetime**: Whole conversation
- **Model tier**: Flagship (Claude Opus / GPT-4o)
- **What it does**: Takes user messages, decides if it's a question (answer in chat) or a task (draft a plan). Handles mid-run messages while plan is executing.
- **Commands it accepts**:
  - `UserMessage { text, reply_tx }` → returns `ConductorReply`
    - `CriticVerdict { plan_id, version, verdict }` → processes critic feedback
      - `EndConversation` → emits `ConversationEnded`, shuts down
      - **Replies it returns**:
        - `Chat { reply }` — plain text answer
          - `PlanDrafted { plan_id, version }` — a plan was created
            - `PlanRevised { plan_id, version }` — plan updated after critic feedback
              - `HotPatched { task, target_plan }` — task injected mid-run
                - `ScopeChanged { new_plan_id }` — plan replaced entirely
                  - `Awaiting { plan_id }` — waiting for user approval
                    - `Error { message }` — something went wrong
                    - **Mid-run classifier**: When user sends a message while a plan is running, Conductor classifies it:
                      - `informational` — just answer the question
                        - `hot_patch` — add a new task to the running plan
                          - `scope_change` — stop everything, draft new plan
                            - `new_plan` — unrelated ask, draft from scratch

                            ### 2. Critic (blind one-shot reviewer)
                            - **Lifetime**: One invocation per plan version
                            - **Model tier**: Flagship (same as Conductor)
                            - **What it does**: Reviews the plan (tasks, sandbox config, models) without seeing conversation history. Posts verdict to the bus.
                            - **No tools** — pure text in, structured verdict out
                            - **Verdict types**:
                              - `Approve` — plan is sound
                                - `Revise { issues }` — plan has issues, Conductor should fix
                                  - `Block { reason, issues }` — fundamental problems, user must override
                                  - **Issue structure**:
                                    - `severity`: "info" | "warn" | "block"
                                      - `section`: "tasks.toml" | "models.toml" | "sandbox.toml" | "overview.md"
                                        - `message`: description
                                          - `suggested_fix`: optional fix

                                          ### 3. Captain (execution orchestrator)
                                          - **Lifetime**: Whole conversation (idle when no plan running)
                                          - **Model tier**: Mid (Claude Sonnet / GPT-4o-mini)
                                          - **What it does**: Manages plan execution. Spawns Crews, tracks progress, handles failures/retries, merges overlays.
                                          - **Commands it accepts**:
                                            - `StartPlan { plan_id, reply_tx }` → returns `CaptainOutcome`
                                              - `InjectTask { task, reply_tx }` → adds task mid-run
                                                - `PauseAll { reply_tx }` — pause all in-flight crews
                                                  - `ResumeAll { reply_tx }` — resume paused crews
                                                    - `CancelPlan { reason, reply_tx }` — abort everything
                                                      - `QueryStatus { reply_tx }` → returns `CaptainStatus`
                                                        - `Shutdown` — clean up and exit
                                                        - **Status struct** (`CaptainStatus`):
                                                          - `plan_id`: current plan
                                                            - `sandbox_id`: VM sandbox
                                                              - `in_flight`: `Vec<(TaskId, agent_id, tool_calls_so_far)>`
                                                                - `completed`: `Vec<TaskId>`
                                                                  - `failed`: `Vec<(TaskId, reason)>`
                                                                  - **Outcomes**:
                                                                    - `Completed { sandbox_id }` — all tasks done
                                                                      - `Failed { reason, sandbox_id }` — something broke
                                                                        - `Aborted` — user cancelled

                                                                        ### 4. Crew (per-task fast agent)
                                                                        - **Lifetime**: Until one task completes
                                                                        - **Model tier**: Fast (Haiku / GPT-4o-mini / Gemini Flash)
                                                                        - **What it does**: Executes a single task. Reads files, writes code, runs commands, commits. Emits `CrewToolSummary` for every tool call.
                                                                        - **Tool budget**: Max tool calls before Captain steps in (configurable per task)
                                                                        - **Available tools**: `read_file`, `write_file`, `list_files`, `run_command`, `grep`, `find_files`, `git_status`, `git_add`, `git_commit`, `git_diff`, `http_get`, `http_post`, `append_file`, `delete_file`, `create_directory`
                                                                        - **Outcomes**:
                                                                          - `Done { agent_id, summary, tool_calls }` — task completed
                                                                            - `Failed { agent_id, reason, tool_calls }` — task failed

                                                                            ---

                                                                            ## EVENTS (everything that flows on the bus)

                                                                            These are what the frontend subscribes to:

                                                                            ### From Conductor
                                                                            | Event | When | Fields |
                                                                            |---|---|---|
                                                                            | `PlanDrafted` | Plan created | `plan_id`, `version` |
                                                                            | `PlanRevised` | Plan updated after critic | `plan_id`, `version` |
                                                                            | `PlanSuperseded` | Old plan replaced | `old_plan_id`, `new_plan_id` |
                                                                            | `ConductorReply` | Any reply to user | `kind` (Chat/PlanDrafted/...), `text` |

                                                                            ### From Critic
                                                                            | Event | When | Fields |
                                                                            |---|---|---|
                                                                            | `CriticReviewing` | Critic started | `plan_id`, `version` |
                                                                            | `CriticVerdict` | Critic finished | `plan_id`, `version`, `verdict` |

                                                                            ### From User (via host)
                                                                            | Event | When | Fields |
                                                                            |---|---|---|
                                                                            | `UserApproved` | User approved plan | `plan_id` |
                                                                            | `UserOverride` | User overrode critic block | `plan_id` |
                                                                            | `UserCancelled` | User cancelled plan | `plan_id` |
                                                                            | `UserMessageReceived` | Any user message | `text` |

                                                                            ### From Captain
                                                                            | Event | When | Fields |
                                                                            |---|---|---|
                                                                            | `CaptainStarted` | Captain begins execution | `plan_id`, `sandbox_id` |
                                                                            | `CrewStarted` | Crew spawned for task | `plan_id`, `task_id`, `agent_id` |
                                                                            | `CrewToolSummary` | **Every single tool call** | `agent_id`, `text`, `tool_name`, `success` |
                                                                            | `CrewDone` | Crew finished task | `plan_id`, `agent_id`, `summary` |
                                                                            | `CrewFailed` | Crew failed task | `plan_id`, `agent_id`, `reason` |
                                                                            | `MergeStarted` | Merge begun | `plan_id`, `agent_id` |
                                                                            | `MergeCompleted` | Merge finished | `plan_id`, `agent_id` |
                                                                            | `TaskInjected` | Hot-patched task added | `plan_id`, `task_id`, `reason` |
                                                                            | `PlanCompleted` | All tasks done | `plan_id` |
                                                                            | `PlanFailed` | Plan aborted/failed | `plan_id`, `reason` |
                                                                            | `CaptainStatusUpdate` | Status query response | `status` (CaptainStatus struct) |

                                                                            ### Lifecycle
                                                                            | Event | When | Fields |
                                                                            |---|---|---|
                                                                            | `ConversationEnded` | User typed /end | — |
                                                                            | `CaptainShutdown` | Captain cleaned up | `sandbox_id`, `final_plan_status` |

                                                                            ---

                                                                            ## PLAN FORMAT (on disk)

                                                                            Stored at `.mowis/plans/<plan_id>/`:

                                                                            ```
                                                                            plan.toml          — metadata (plan_id, status, user_goal, current_version)
                                                                            overview.md        — Conductor's prose description
                                                                            tasks.toml         — task graph (id, title, description, deps, model_tier, tool_budget, files_hint)
                                                                            sandbox.toml       — VM config (image_rootfs, ram_mb, cpu_millis)
                                                                            models.toml        — model assignments per tier + per-task overrides
                                                                            tools.toml         — tool whitelist deltas (allow_extra, deny)
                                                                            status.toml        — Captain's append-only execution log
                                                                            critic/vN.md       — critic's prose review
                                                                            critic/vN.toml     — critic's structured verdict
                                                                            history/vN/        — snapshots of previous plan versions
                                                                            tool_logs/         — raw tool I/O per agent per round
                                                                            ```

                                                                            **Plan statuses**: `draft` → `awaiting_user` → `approved` → `running` → `done` | `aborted`

                                                                            ---

                                                                            ## TASK GRAPH (in tasks.toml)

                                                                            Each task has:
                                                                            - `id` — short stable id (e.g., "t1", "t2")
                                                                            - `title` — short description
                                                                            - `description` — what the crew should do
                                                                            - `deps` — list of task ids this depends on (forms a DAG)
                                                                            - `model_tier` — "fast" | "mid" | "flagship"
                                                                            - `tool_budget` — max tool calls
                                                                            - `files_hint` — advisory list of files the crew may touch

                                                                            ---

                                                                            ## CONFIG (~/.mowisai/mowis.toml)

                                                                            ```toml
                                                                            [providers.anthropic]
                                                                            api_key_enc = "encrypted..."

                                                                            [providers.openai]
                                                                            api_key_enc = "encrypted..."

                                                                            [tier.conductor]
                                                                            provider = "anthropic"
                                                                            model = "claude-opus-4-7"

                                                                            [tier.critic]
                                                                            provider = "anthropic"
                                                                            model = "claude-opus-4-7"

                                                                            [tier.captain]
                                                                            provider = "anthropic"
                                                                            model = "claude-sonnet-4-6"

                                                                            [tier.crew]
                                                                            provider = "anthropic"
                                                                            model = "claude-haiku-4-5-20251001"

                                                                            [sandbox]
                                                                            image_rootfs = ".mowis-cache/rootfs/ubuntu-24.04"
                                                                            ram_mb = 8192
                                                                            cpu_millis = 4000
                                                                            ```

                                                                            ---

                                                                            ## PROTOCOL PAYLOADS (host ↔ VM)

                                                                            ### Host → Guest (requests)
                                                                            | Payload | Purpose |
                                                                            |---|---|
                                                                            | `Ping` | Health check |
                                                                            | `Version` | Get executor version |
                                                                            | `Shutdown` | Kill executor |
                                                                            | `CreateSandbox { sandbox_id, image_rootfs, limits }` | Create overlayfs sandbox |
                                                                            | `DestroySandbox { sandbox_id }` | Tear down sandbox |
                                                                            | `ListSandboxes` | List active sandboxes |
                                                                            | `Exec { sandbox_id, cmd, args, env }` | Run command in sandbox |
                                                                            | `InvokeTool { sandbox_id, tool, input }` | Call a tool |
                                                                            | `CreateAgentOverlay { parent_sandbox_id, agent_id, limits }` | Create agent's overlay |
                                                                            | `MergeAgentOverlay { parent_sandbox_id, agent_id }` | Merge agent's work to parent |
                                                                            | `DiscardAgentOverlay { parent_sandbox_id, agent_id }` | Throw away agent's work |
                                                                            | `InvokeToolAsAgent { parent_sandbox_id, agent_id, tool, input, caller_tier }` | Tool call scoped to agent |
                                                                            | `UploadCodebase { sandbox_id, archive_b64, file_count }` | Upload project files |
                                                                            | `HealthCheck` | Check executor health |

                                                                            ### Guest → Host (responses)
                                                                            | Payload | Purpose |
                                                                            |---|---|
                                                                            | `Pong { version, protocol }` | Ping response |
                                                                            | `SandboxCreated { sandbox_id }` | Sandbox ready |
                                                                            | `SandboxDestroyed { sandbox_id }` | Sandbox torn down |
                                                                            | `SandboxList { sandboxes }` | List of sandboxes |
                                                                            | `Stdout { data }` | Command output |
                                                                            | `Stderr { data }` | Command error output |
                                                                            | `ExitCode { code }` | Command finished |
                                                                            | `ToolResult { output }` | Tool call result |
                                                                            | `Error { message }` | Something failed |
                                                                            | `AgentOverlayCreated { agent_id }` | Overlay ready |
                                                                            | `AgentOverlayMerged { agent_id, changed_paths }` | Overlay merged |
                                                                            | `AgentOverlayDiscarded { agent_id }` | Overlay discarded |
                                                                            | `CodebaseUploaded { sandbox_id, file_count }` | Upload complete |
                                                                            | `HealthOk { uptime_secs, sandbox_count }` | Health check response |

                                                                            ---

                                                                            ## UI STREAMS (what the frontend shows)

                                                                            ### Stream 1: Conductor Chat (always visible, main pane)
                                                                            - `ConductorReply` events
                                                                            - `CriticVerdict` events (labeled as critic message)
                                                                            - `UserMessageReceived` events
                                                                            - `PlanDrafted` / `PlanRevised` (as plan preview/link)
                                                                            - User approval prompts

                                                                            ### Stream 2: Captain Panel (collapsed, click to expand)
                                                                            - `CrewStarted` — "Crew ag-t1 started task t1"
                                                                            - `CrewToolSummary` — "Agent read src/main.rs (4.2 KB)"
                                                                            - `CrewDone` — "Crew ag-t1 done: Added healthz endpoint"
                                                                            - `CrewFailed` — "Crew ag-t1 failed: tool budget exceeded"
                                                                            - `MergeCompleted` — "Merged overlay for ag-t1"
                                                                            - `TaskInjected` — "Task h1 injected: user asked for logging"
                                                                            - `CaptainStatusUpdate` — full status object

                                                                            ### Stream 3: Per-Agent Drill-Down (click agent row to expand)
                                                                            - Same as Captain Panel, filtered to one `agent_id`
                                                                            - Raw tool I/O on demand (fetched from `tool_logs/`)

                                                                            ---

                                                                            ## TOOL SUMMARY TEMPLATES (what CrewToolSummary shows)

                                                                            Each tool call produces exactly one human-readable sentence:

                                                                            | Tool | Success | Failure |
                                                                            |---|---|---|
                                                                            | `run_command` | "Agent ran a terminal command: `npm install`" | "...`npm install` (FAILED: exit code 1)" |
                                                                            | `read_file` | "Agent read src/main.rs (4.2 KB)" | "Agent tried to read src/main.rs (FAILED: file not found)" |
                                                                            | `write_file` | "Agent wrote 12 lines to src/routes.rs" | "Agent tried to write to src/routes.rs (FAILED: ...)" |
                                                                            | `list_files` | "Agent listed 8 entries in src/" | "Agent tried to list src/ (FAILED: ...)" |
                                                                            | `grep` | "Agent searched for `fn main` in src/ (3 matches)" | "Agent tried to search for `fn main` (FAILED: ...)" |
                                                                            | `git_commit` | "Agent committed: 'feat: add healthz' (3 files)" | "Agent tried to commit (FAILED: ...)" |
                                                                            | `git_diff` | "Agent diffed src/" | "Agent diffed src/ (FAILED: ...)" |
                                                                            | `git_status` | "Agent checked git status" | "...git status (FAILED: ...)" |
                                                                            | `http_get` | "Agent fetched GET https://api.example.com (200)" | "Agent tried to fetch GET ... (FAILED: ...)" |
                                                                            | `delete_file` | "Agent deleted old.txt" | "Agent tried to delete old.txt (FAILED: ...)" |
                                                                            | `create_directory` | "Agent created directory src/utils" | "...(FAILED: ...)" |
                                                                            | `run_script` | "Agent ran script setup.sh" | "Agent script setup.sh failed (FAILED: ...)" |
                                                                            | `copy_file` | "Agent copied a.txt to b.txt" | "...(FAILED: ...)" |
                                                                            | Any unknown tool | "Agent invoked `custom_tool`" | "Agent invoked `custom_tool` (FAILED: ...)" |
                                                                            | Denied tool | — | "Agent invoked `rm -rf` (DENIED by tool whitelist)" |

                                                                            ---

                                                                            ## MERGER

                                                                            - Watches all agent completions
                                                                            - Records `agent_id → changed_paths`
                                                                            - `merge_all(base_path)` — detects conflicts (same file changed by multiple agents)
                                                                            - `promote_agent(upper_dir, base_dir, agent_id)` — copies agent's work to base
                                                                            - Emits `MergeCompleted` after each promotion
                                                                            - Conflicts are surfaced to Captain LLM for resolution

                                                                            ---

                                                                            ## WHAT THE FRONTEND NEEDS TO DO

                                                                            1. **Connect to mowisd** — send `ConductorCommand::UserMessage` via stdin or IPC
                                                                            2. **Subscribe to event bus** — receive all events, filter by stream
                                                                            3. **Show Conductor Chat** — render `ConductorReply` and `CriticVerdict` inline
                                                                            4. **Show Plan Preview** — when `PlanDrafted` fires, load plan files from `.mowis/plans/<id>/` and render tasks.toml as a visual DAG
                                                                            5. **Approval flow** — when plan is drafted, show Approve/Cancel buttons, emit `UserApproved` or `UserCancelled`
                                                                            6. **Captain Panel** — show `CrewStarted`/`CrewToolSummary`/`CrewDone`/`CrewFailed` as a live activity feed
                                                                            7. **Per-agent drill-down** — click an agent to see its filtered tool summary stream
                                                                            8. **Status queries** — send `CaptainCommand::QueryStatus` to get current state
                                                                            9. **Mid-run injection** — user types message while plan is running, Conductor classifies it
                                                                            10. **End conversation** — send `ConductorCommand::EndConversation`

                                                                            ---

                                                                            ## SANDBOX ARCHITECTURE

                                                                            ```
                                                                            VM (Alpine Linux)
                                                                              └─ mowis-executor (PID 1, vsock server)

                                                                              Sandbox (per conversation):
                                                                                lower dir = user's codebase (read-only)
                                                                                  upper dir = empty (read-write)

                                                                                  Agent Overlay (per crew):
                                                                                    lower dir = sandbox merged view (read-only)
                                                                                      upper dir = agent's work (read-write)

                                                                                      Flow:
                                                                                        1. User codebase uploaded → lower dir of sandbox
                                                                                          2. Captain creates N agent overlays
                                                                                            3. Each crew reads from lower, writes to upper
                                                                                              4. On crew completion: merge upper → parent sandbox
                                                                                                5. All agents see updated base after merge
                                                                                                ```

                                                                                                ---

                                                                                                ## CONVERSATION LIFECYCLE

                                                                                                ```
                                                                                                1. User types message
                                                                                                   → Conductor receives UserMessage
                                                                                                      → If no plan: drafts plan or answers question
                                                                                                         → If plan running: classifies (informational/hot_patch/scope_change/new_plan)

                                                                                                         2. Plan drafted
                                                                                                            → Critic reviews in parallel with user reading
                                                                                                               → Critic posts verdict to bus
                                                                                                                  → Conductor processes verdict (may revise)

                                                                                                                  3. User approves
                                                                                                                     → Captain receives StartPlan
                                                                                                                        → Captain creates sandbox (uploads codebase if provided)
                                                                                                                           → Captain computes topological sort of task graph
                                                                                                                              → Captain spawns crews for ready tasks (parallel waves)

                                                                                                                              4. Crew execution
                                                                                                                                 → Each crew runs LLM tool-calling loop
                                                                                                                                    → Every tool call: CrewToolSummary emitted
                                                                                                                                       → Tool calls go over vsock to VM executor
                                                                                                                                          → On success: crew emits CrewDone, overlay merged
                                                                                                                                             → On failure: Captain retries up to 2 times

                                                                                                                                             5. Mid-run injection
                                                                                                                                                → User sends message while plan running
                                                                                                                                                   → Conductor classifies
                                                                                                                                                      → If hot_patch: Captain injects new task
                                                                                                                                                         → If scope_change: Captain pauses, new plan drafted

                                                                                                                                                         6. Plan completes
                                                                                                                                                            → All crews done, all merges complete
                                                                                                                                                               → Captain emits PlanCompleted
                                                                                                                                                                  → User can continue chatting or type /end

                                                                                                                                                                  7. Conversation ends
                                                                                                                                                                     → Captain destroys sandbox
                                                                                                                                                                        → Captain emits CaptainShutdown
                                                                                                                                                                           → Process exits
                                                                                                                                                                           ```
                                                                                                                                                                           