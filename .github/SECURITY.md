# Security Policy

## Supported Versions

We release patches for security vulnerabilities in the following versions:

| Version | Supported          |
| ------- | ------------------ |
| 0.2.x   | :white_check_mark: |
| 0.1.x   | :x:                |

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Instead, please report them via email to security@mowisai.com.

You should receive a response within 48 hours. If for some reason you do not, please follow up via email to ensure we received your original message.

Please include the following information in your report:

- Type of issue (e.g., buffer overflow, SQL injection, cross-site scripting, etc.)
- Full paths of source file(s) related to the manifestation of the issue
- The location of the affected source code (tag/branch/commit or direct URL)
- Any special configuration required to reproduce the issue
- Step-by-step instructions to reproduce the issue
- Proof-of-concept or exploit code (if possible)
- Impact of the issue, including how an attacker might exploit it

This information will help us triage your report more quickly.

## Security Features

agentd is designed with security as a core principle:

### Sandboxing
- **Overlayfs isolation**: Each agent runs in an isolated filesystem layer
- **Chroot jails**: Agents cannot access files outside their sandbox
- **Cgroups**: Resource limits prevent resource exhaustion attacks
- **Seccomp filters**: System call filtering blocks dangerous operations

### Compliance
- **GDPR compliant**: No data leaves the local system
- **DORA ready**: Designed for European regulated environments
- **Audit logging**: All agent actions are logged for compliance

### Cryptography
- **AES-GCM encryption**: Sensitive data is encrypted at rest
- **SHA-256 hashing**: Integrity verification for checkpoints
- **Secure random generation**: Cryptographically secure randomness

### Network Security
- **Unix socket communication**: No network exposure by default
- **No cloud dependency**: All processing happens locally
- **Input validation**: All external inputs are validated and sanitized

## Security Best Practices

When using agentd:

1. **Run with minimal privileges**: Only the socket server requires root access
2. **Keep dependencies updated**: Regularly update Rust and system packages
3. **Monitor resource usage**: Set appropriate cgroup limits
4. **Review audit logs**: Regularly check logs for suspicious activity
5. **Isolate sensitive data**: Use separate sandboxes for different security contexts
6. **Validate tool outputs**: Don't trust agent outputs without verification

## Known Security Considerations

### Root Requirement
The socket server requires root access for overlayfs mounts. This is a fundamental requirement of the overlayfs filesystem. We mitigate this by:
- Dropping privileges after mount operations
- Using seccomp to restrict system calls
- Isolating the socket server from agent execution

### Resource Exhaustion
While cgroups provide resource limits, a malicious or buggy agent could still attempt resource exhaustion. We mitigate this by:
- File descriptor limits (65536 per process)
- Connection pool bounds (SLOW_WORKERS * 3/4)
- Automatic cleanup of failed agents
- Performance gates in CI/CD

### Checkpoint Security
Checkpoints contain agent state and could include sensitive data. We mitigate this by:
- Encrypting checkpoints at rest
- Automatic cleanup after task completion
- Secure deletion of temporary files

## Security Updates

Security updates will be released as soon as possible after a vulnerability is confirmed. Updates will be announced via:
- GitHub Security Advisories
- Release notes
- Email to registered users (if applicable)

## Bug Bounty Program

We currently do not have a bug bounty program. However, we deeply appreciate security researchers who responsibly disclose vulnerabilities to us.

## Contact

For security concerns, contact: security@mowisai.com

For general questions: info@mowisai.com
