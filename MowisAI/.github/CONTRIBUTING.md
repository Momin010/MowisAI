# Contributing to MowisAI agentd

Thank you for your interest in contributing to agentd! This document provides guidelines and instructions for contributing.

## Code of Conduct

By participating in this project, you agree to maintain a respectful and inclusive environment for all contributors.

## Getting Started

1. **Fork the repository** on GitHub
2. **Clone your fork** locally:
   ```bash
   git clone https://github.com/YOUR_USERNAME/agentd.git
   cd agentd
   ```
3. **Add upstream remote**:
   ```bash
   git remote add upstream https://github.com/mowisai/agentd.git
   ```
4. **Create a branch** for your changes:
   ```bash
   git checkout -b feature/your-feature-name
   ```

## Development Setup

### Prerequisites
- Linux (overlayfs and cgroups required)
- Rust stable toolchain
- Root access for testing
- gcloud CLI (for Vertex AI integration)

### Build
```bash
cargo build
```

### Run Tests
```bash
cargo test
```

All 67 tests must pass. Never delete or modify tests to make them pass.

### Run Locally
```bash
# Terminal 1 - Socket server (requires root)
sudo ./target/debug/agentd socket --path /tmp/agentd.sock

# Terminal 2 - Run simulation
./target/debug/agentd simulate \
    --socket /tmp/agentd.sock \
    --project-root /path/to/test/project \
    --max-agents 10 \
    --tasks 20
```

## Making Changes

### Code Style
- Follow Rust standard style guidelines
- Run `cargo fmt` before committing
- Run `cargo clippy` and fix all warnings
- Add comments for complex logic
- Use meaningful variable and function names

### Hard Invariants
These must NEVER be violated:

1. **Orchestrator-mediated coordination only** - No direct agent-to-agent communication
2. **Type safety** - Sandbox and container IDs are always String in JSON, never u64
3. **Test integrity** - Never delete or modify tests to make them pass
4. **No stubbing** - Never stub or fake tool implementations
5. **Container context** - All tools execute within container context via chroot
6. **Test coverage** - All 67 tests must always pass
7. **Error handling** - No unwrap() in production code paths

### Commit Messages
Follow conventional commits format:

```
type(scope): subject

body

footer
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, etc.)
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `test`: Test changes
- `chore`: Build process or auxiliary tool changes

Example:
```
feat(orchestration): add parallel merge optimization

Implement tree-pattern merge to reduce merge rounds from N to log2(N).
This significantly improves performance for large task graphs.

Closes #123
```

### Testing
- Add tests for all new features
- Add tests for all bug fixes
- Ensure all existing tests pass
- Run integration tests when changing core functionality
- Test with the performance gate for performance-sensitive changes

### Documentation
- Update README.md for user-facing changes
- Add inline comments for complex logic
- Update CLAUDE.md for architectural changes
- Add examples for new features

## Pull Request Process

1. **Update your branch** with the latest upstream changes:
   ```bash
   git fetch upstream
   git rebase upstream/main
   ```

2. **Run all checks**:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-features
   ```

3. **Push your changes**:
   ```bash
   git push origin feature/your-feature-name
   ```

4. **Create a Pull Request** on GitHub with:
   - Clear title and description
   - Reference to related issues
   - Description of changes made
   - Testing performed
   - Screenshots/examples if applicable

5. **Address review feedback**:
   - Make requested changes
   - Push updates to your branch
   - Respond to comments

6. **Merge requirements**:
   - All CI checks must pass
   - At least one approval from a maintainer
   - No unresolved conversations
   - Branch must be up to date with main

## Areas for Contribution

### High Priority
- Performance optimizations
- Additional tool implementations
- Security enhancements
- Documentation improvements
- Test coverage expansion

### Good First Issues
Look for issues labeled `good-first-issue` for beginner-friendly tasks.

### Tools
New tools should:
- Execute within container context
- Handle errors gracefully
- Include comprehensive tests
- Follow the existing tool pattern
- Be documented with examples

### Orchestration
Changes to orchestration should:
- Maintain the 7-layer architecture
- Preserve hard invariants
- Pass performance gates
- Include simulation tests

### Security
Security contributions should:
- Follow secure coding practices
- Include threat analysis
- Be reviewed by security team
- Include security tests

## Performance Considerations

- Run performance gate before submitting: `bash scripts/perf-gate.sh`
- Profile changes with `cargo bench`
- Avoid blocking operations in hot paths
- Use async/await appropriately
- Consider memory allocation patterns

## Getting Help

- **Questions**: Open a GitHub Discussion
- **Bugs**: Open a GitHub Issue
- **Security**: Email security@mowisai.com
- **General**: Email info@mowisai.com

## Recognition

Contributors will be recognized in:
- Release notes
- CONTRIBUTORS.md file
- GitHub contributors page

Thank you for contributing to agentd! 🚀
