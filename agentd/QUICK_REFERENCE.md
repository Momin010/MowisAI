# AgentD Quick Reference - 75 Tools Guide

## Quick Navigation

### By Category
- [Filesystem (11)](#filesystem-tools) | [Shell (5)](#shell-tools) | [HTTP (7)](#http-tools)
- [Data (5)](#data-tools) | [Git (9)](#git-tools) | [Docker (7)](#docker-tools)
- [Kubernetes (6)](#kubernetes-tools) | [Storage (8)](#storage-tools) | [PackageManagers (3)](#package-managers)
- [Web (3)](#web-tools) | [Channels (5)](#channels-tools) | [Dev Tools (6)](#dev-tools)

### By Use Case
- **File Management** → Filesystem Tools
- **Automation** → Shell + Dev Tools
- **API Integration** → HTTP Tools
- **Data Processing** → Data + CSV Tools
- **Version Control** → Git Tools
- **Containerization** → Docker Tools
- **Orchestration** → Kubernetes Tools
- **Agent Communication** → Channels Tools
- **Data Persistence** → Storage Tools

---

## Filesystem Tools

| Tool | Params | Returns | Quick Use |
|------|--------|---------|-----------|
| **ReadFile** | `path` | content | Read files |
| **WriteFile** | `path`, `content` | success | Create/overwrite |
| **AppendFile** | `path`, `content` | success | Append to file |
| **DeleteFile** | `path` | success | Delete file |
| **CopyFile** | `from`, `to` | success | Copy file |
| **MoveFile** | `from`, `to` | success | Move/rename |
| **ListFiles** | `path` | files, dirs | List directory |
| **CreateDirectory** | `path` | success | Make dir(s) |
| **DeleteDirectory** | `path` | success | Remove dir tree |
| **GetFileInfo** | `path` | metadata | File stats |
| **FileExists** | `path` | boolean | Check exists |

### Example: Read a File
```json
{
  "tool": "read_file",
  "params": {"path": "/data.txt"}
}
```

---

## Shell Tools

| Tool | Params | Returns | Quick Use |
|------|--------|---------|-----------|
| **RunCommand** | `cmd`, `cwd?` | stdout/err | Execute command |
| **RunScript** | `path`, `interpreter?` | output | Run script |
| **KillProcess** | `pid`, `signal?` | success | Kill process |
| **GetEnv** | `var` | value | Get env var |
| **SetEnv** | `var`, `value` | success | Set env var |

### Example: Run Command
```json
{
  "tool": "run_command",
  "params": {"cmd": "ls -la /tmp"}
}
```

---

## HTTP Tools

| Tool | Params | Returns | Quick Use |
|------|--------|---------|-----------|
| **HttpGet** | `url` | response | GET request |
| **HttpPost** | `url`, `body` | response | POST request |
| **HttpPut** | `url`, `body` | response | PUT request |
| **HttpDelete** | `url` | response | DELETE request |
| **HttpPatch** | `url`, `body` | response | PATCH request |
| **DownloadFile** | `url`, `path` | success | Download file |
| **WebsocketSend** | `url`, `message` | success | WebSocket msg |

### Example: HTTP POST
```json
{
  "tool": "http_post",
  "params": {
    "url": "https://api.example.com/data",
    "body": "{\"name\": \"alice\"}"
  }
}
```

---

## Data Tools

| Tool | Params | Returns | Quick Use |
|------|--------|---------|-----------|
| **JsonParse** | `data` | object | Parse JSON |
| **JsonStringify** | `data` | string | JSON encode |
| **JsonQuery** | `data`, `query` | value | Extract JPath |
| **CsvRead** | `path`, `delimiter?` | rows | Read CSV |
| **CsvWrite** | `path`, `data` | success | Write CSV |

### Example: Parse JSON
```json
{
  "tool": "json_parse",
  "params": {"data": "{\"key\": \"value\"}"}
}
```

---

## Git Tools

| Tool | Params | Returns | Quick Use |
|------|--------|---------|-----------|
| **GitClone** | `url`, `path` | output | Clone repo |
| **GitStatus** | `path` | status | Show status |
| **GitAdd** | `path`, `files[]` | success | Stage files |
| **GitCommit** | `path`, `message` | hash | Create commit |
| **GitPush** | `path`, `remote?`, `branch?` | output | Push to remote |
| **GitPull** | `path`, `remote?` | output | Pull changes |
| **GitBranch** | `path`, `action`, `name?` | branches | Create/list branch |
| **GitCheckout** | `path`, `branch` | success | Switch branch |
| **GitDiff** | `path`, `file1?`, `file2?` | diff | Show diff |

### Example: Commit Changes
```json
{
  "tool": "git_commit",
  "params": {
    "path": "/repo",
    "message": "Add new feature"
  }
}
```

---

## Docker Tools

| Tool | Params | Returns | Quick Use |
|------|--------|---------|-----------|
| **DockerBuild** | `dockerfile`, `tag` | output | Build image |
| **DockerRun** | `image`, `name`, `ports?` | container_id | Run container |
| **DockerStop** | `container_id` | success | Stop container |
| **DockerPs** | `all?` | containers | List running |
| **DockerLogs** | `container_id`, `lines?` | logs | Get logs |
| **DockerExec** | `container_id`, `cmd` | output | Run in container |
| **DockerPull** | `image` | output | Pull image |

### Example: List Containers
```json
{
  "tool": "docker_ps",
  "params": {"all": true}
}
```

---

## Kubernetes Tools

| Tool | Params | Returns | Quick Use |
|------|--------|---------|-----------|
| **KubectlApply** | `manifest` | output | Apply manifest |
| **KubectlGet** | `resource_type`, `namespace?` | resources | List resources |
| **KubectlDelete** | `resource_type`, `resource_name` | output | Delete resource |
| **KubectlLogs** | `pod_name`, `namespace?` | logs | Get pod logs |
| **KubectlExec** | `pod_name`, `cmd` | output | Run in pod |
| **KubectlDescribe** | `resource_type`, `resource_name` | details | Get details |

### Example: Get Pods
```json
{
  "tool": "kubectl_get",
  "params": {
    "resource_type": "pods",
    "namespace": "default"
  }
}
```

---

## Storage Tools

| Tool | Params | Returns | Quick Use |
|------|--------|---------|-----------|
| **MemorySet** | `key`, `value` | success | Store in memory |
| **MemoryGet** | `key` | value | Get from memory |
| **MemoryDelete** | `key` | success | Delete from memory |
| **MemoryList** | (none) | keys[] | List all keys |
| **MemorySave** | `path` | success | Save to disk |
| **MemoryLoad** | `path` | success | Load from disk |
| **SecretSet** | `key`, `value` | success | Store secret |
| **SecretGet** | `key` | value | Get secret |

### Example: Store Secret
```json
{
  "tool": "secret_set",
  "params": {
    "key": "api_token",
    "value": "secret_xyz"
  }
}
```

---

## Package Managers

| Tool | Params | Returns | Quick Use |
|------|--------|---------|-----------|
| **NpmInstall** | `package`, `version?` | output | Install npm pkg |
| **PipInstall** | `package`, `version?` | output | Install python pkg |
| **CargoAdd** | `crate`, `version?` | output | Add rust crate |

### Example: Install NPM Package
```json
{
  "tool": "npm_install",
  "params": {
    "package": "express",
    "version": "^4.18.0"
  }
}
```

---

## Web Tools

| Tool | Params | Returns | Quick Use |
|------|--------|---------|-----------|
| **WebSearch** | `query`, `limit?` | results[] | Search web |
| **WebFetch** | `url`, `format?` | content | Fetch page |
| **WebScreenshot** | `url`, `format?` | image | Screenshot page |

### Example: Search Web
```json
{
  "tool": "web_search",
  "params": {
    "query": "rust programming language",
    "limit": 10
  }
}
```

---

## Channels Tools

| Tool | Params | Returns | Quick Use |
|------|--------|---------|-----------|
| **CreateChannel** | `name` | channel_id | Create channel |
| **SendMessage** | `channel`, `message` | msg_id | Send message |
| **ReadMessages** | `channel`, `limit?` | messages[] | Read messages |
| **Broadcast** | `message` | success | Broadcast to all |
| **WaitFor** | `channel`, `timeout?` | messages[] | Wait for events |

### Example: Send Message
```json
{
  "tool": "send_message",
  "params": {
    "channel": "alerts",
    "message": "System update available"
  }
}
```

---

## Dev Tools

| Tool | Params | Returns | Quick Use |
|------|--------|---------|-----------|
| **Echo** | `message` | echo | Print message |
| **SpawnAgent** | `agent_id`, `config?` | handle | Create agent |
| **Lint** | `path`, `language?` | issues[] | Lint code |
| **Test** | `path`, `filter?` | results | Run tests |
| **Build** | `path`, `release?` | output | Build project |
| **TypeCheck** | `path`, `language?` | errors[] | Type check |

### Example: Build Project
```json
{
  "tool": "build",
  "params": {
    "path": "/myproject",
    "release": true
  }
}
```

---

## Common Patterns

### Sequential Operations: File → Git → Push
```
1. WriteFile → Create local file
2. GitAdd → Stage the file
3. GitCommit → Commit with message
4. GitPush → Push to remote
```

### Data Pipeline: Fetch → Parse → Store
```
1. WebFetch → Get data from URL
2. JsonParse → Parse the content
3. MemorySet → Store in memory
4. CsvWrite → Write to CSV
```

### Deployment: Build → Docker → Kubernetes
```
1. Build → Build application
2. DockerBuild → Build image
3. DockerPush → Push to registry
4. KubectlApply → Deploy to K8s
```

---

## Error Handling

Most tools return:
- `success: boolean` - Operation success
- `error?: string` - Error message if failed
- `data?: any` - Result data if successful

### Example with Error Handling
```json
{
  "tool": "read_file",
  "params": {"path": "/missing.txt"}
}

// Response:
{
  "success": false,
  "error": "File not found: /missing.txt"
}
```

---

## Performance Tips

1. **Batch Operations:** Use channels for bulk messaging
2. **Cache Data:** Use MemorySet for frequently accessed data
3. **Parallel Execution:** Spawn multiple agents for parallel work
4. **Resource Limits:** Set limits when creating sandboxes
5. **Cleanup:** Use DeleteFile/DeleteDirectory to manage space

---

## Security Notes

- **Secrets:** Use SecretSet/SecretGet for sensitive data (encrypted)
- **Environment:** Separate production/dev via SetEnv
- **Access Control:** Validate inputs before tool invocation
- **Sandboxing:** All tools run in isolated sandbox context
- **Logging:** Audit trails available through persistence

---

## Testing Your Tools

Run comprehensive test suite:
```bash
cd /workspaces/MowisAI/agentd
cargo test --test comprehensive_integration_tests -- --nocapture
```

Run specific tool category tests:
```bash
cargo test test_filesystem_tool_suite -- --nocapture
cargo test test_git_tool_suite -- --nocapture
```

---

## Quick Reference: Tool Count by Category

| Category | Count | Status |
|----------|-------|--------|
| Filesystem | 11 | ✓ |
| Shell | 5 | ✓ |
| HTTP | 7 | ✓ |
| Data | 5 | ✓ |
| Git | 9 | ✓ |
| Docker | 7 | ✓ |
| Kubernetes | 6 | ✓ |
| Storage | 8 | ✓ |
| Package Managers | 3 | ✓ |
| Web | 3 | ✓ |
| Channels | 5 | ✓ |
| Dev Tools | 6 | ✓ |
| **TOTAL** | **75** | **✓** |

---

## Additional Resources

- **Full Documentation:** See `/workspaces/MowisAI/agentd/TOOL_INVENTORY.md`
- **Test Report:** See `/workspaces/MowisAI/agentd/TEST_REPORT.md`
- **Test Code:** See `/workspaces/MowisAI/agentd/tests/comprehensive_integration_tests.rs`
- **Source Code:** See `/workspaces/MowisAI/agentd/src/tools/`

---

**Last Updated:** 2026-03-07 | **Tools:** 75/75 ✓ | **Status:** Production Ready
