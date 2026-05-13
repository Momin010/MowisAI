# Massive Refactor Plan: End-to-End Streaming Multi-Agent Orchestration

## Problem Statement
Current architecture hides execution internals until completion. User sees planning text, then apparent stall.

## Required Outcomes
1. Every operation emits structured runtime event.
2. Desktop renders in-chat live agent board with drill-down logs.
3. Handoff prompt execution transitions into explicit orchestration session with lifecycle states.
4. Multi-agent state machine supports hundreds/thousands of agents with backpressure.

## Backend Refactor (agentd)
- Add `RuntimeEventBus` (fanout, ring buffer, replay by session_id).
- Emit event on: LLM request start/end, tool call start/end, file read/write/create/delete, command start/stdout/stderr/end, sandbox create/mount/checkpoint/rollback, merge start/conflict/resolve/end, verification round start/end.
- Add stream transport over socket using newline-delimited JSON with heartbeat and resume cursor.
- Introduce orchestrator lifecycle states:
  - received
  - classified
  - planned
  - sandbox_provisioning
  - agents_spawning
  - running
  - verifying
  - merging
  - completed/failed/cancelled
- Add backpressure handling:
  - per-client queue cap
  - drop policy only for verbose stdout chunks, never state transitions
- Add durable event store for session replay.

## Desktop Refactor (mowis-desktop)
- Replace sidebar agents panel with in-chat orchestration card.
- Agent matrix cells:
  - gray idle
  - green running
  - green+check completed
  - red error
- On cell click: expandable live log thread in chat.
- File operation events become clickable items, opening diff panel on sidebar.
- Show orchestration phases with timestamps and durations.
- Show stalled-state detector if no non-heartbeat events for N seconds.

## Handoff Redesign
- Main chat LLM outputs PRD + execution contract JSON.
- Contract includes:
  - objective
  - constraints
  - acceptance tests
  - risk profile
  - topology hints
- Orchestrator validates contract, creates orchestration session, emits `handoff.accepted`.
- Worker swarms derive context from workspace scan and contract, not raw chat transcript.

## Implementation Order
1. Event Bus + protocol stabilization
2. Orchestrator lifecycle instrumentation
3. Tool/file/command instrumentation
4. Socket streaming + replay cursor
5. Desktop in-chat board migration
6. Diff-panel deep links
7. Stalled detector + recovery UX

## Non-Goals (phase 1)
- Full redesign of sessions/history pages.
- Replacing existing model provider stack.

## Acceptance Criteria
- User sees first runtime event < 1s after handoff.
- User sees continuous updates during execution.
- No “silent 5-minute hang” without explicit stalled indicator.
- Clicking agent reveals last 200 events.
- Clicking file event opens diff for that file.
