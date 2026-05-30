# MowisAI

An AI agent orchestration engine that runs isolated agents in parallel using QEMU VMs and overlayfs sandboxes.

## Status

**This project is archived and no longer maintained.**

This repository is kept publicly available for anyone who wants to explore, learn from, or use any part of the codebase. You are free to fork it, copy parts of it, or build on top of it. No further updates, bug fixes, or support will be provided.

## What's Here

The codebase contains a four-tier agent orchestration system (Conductor → Critic → Captain → Crew) built in Rust, with:

- **mowis-orchestration/** — Core engine: LLM planning, task scheduling, tool execution, 7 provider integrations (Anthropic, OpenAI, Gemini, Vertex, Grok, Groq, Mimo)
- **mowis-protocol/** — Wire protocol between host and guest VM
- **mowis-host/** — Host-side CLI, QEMU VM lifecycle, TUI
- **mowis-executor/** — Guest-side executor with overlayfs sandbox, tool registry
- **mowis-desktop/** — Desktop GUI built with Tauri
- **agentd/** — Legacy agent daemon with 28 tool integrations (filesystem, shell, git, HTTP, Docker, Kubernetes, GitHub, Jira, Slack, and more)

Total: ~68,000 lines of Rust, ~7,000 lines of JavaScript/CSS, ~1,000 lines of tests.

## Open Source

Everything in this repository is open source under the MIT License. You can:

- Use any part of the code in your own projects
- Fork and modify as you see fit
- Build commercial products on top of it
- Learn from the architecture and implementation decisions

No attribution required (MIT License), but appreciated.

## If You Want to Build It

```bash
# Build the host CLI
cargo build --release -p mowis-host

# Build the executor (for guest VM)
cargo build --release -p mowis-executor

# Run tests
cargo test --workspace
```

Note: The host requires Linux with KVM support for VM-based execution. The executor runs inside the guest VM.

## Why It's Archived

The project explored an ambitious architecture (four-tier orchestration with QEMU VMs) but the complexity-to-value ratio didn't justify continued development. Simpler approaches (single-agent CLI tools like Aider, or IDE-integrated agents like Cursor) achieve similar results with significantly less infrastructure overhead.

The code remains available for anyone interested in:
- Rust-based LLM integration patterns
- QEMU VM lifecycle management
- Overlayfs sandbox implementation
- Multi-provider LLM streaming
- Event-driven agent orchestration
- TUI application design with ratatui

## License

MIT
