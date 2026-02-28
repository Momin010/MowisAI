// Streaming Module
// Handles WebSocket and Server-Sent Events (SSE) for real-time updates

import EventEmitter from 'events';

export class StreamingManager extends EventEmitter {
  constructor() {
    super();
    this.connections = new Map();
    this.taskStreams = new Map();
    this.maxConnections = 100;
  }

  // Register a streaming connection (WebSocket or SSE)
  registerConnection(connectionId, type = 'sse') {
    if (this.connections.size >= this.maxConnections) {
      throw new Error('Max connections reached');
    }

    const connection = {
      id: connectionId,
      type, // 'websocket' or 'sse'
      createdAt: Date.now(),
      isActive: true,
      taskIds: new Set()
    };

    this.connections.set(connectionId, connection);
    return connection;
  }

  // Subscribe connection to task updates
  subscribeToTask(connectionId, taskId) {
    const connection = this.connections.get(connectionId);
    if (!connection) throw new Error('Connection not found');

    connection.taskIds.add(taskId);

    if (!this.taskStreams.has(taskId)) {
      this.taskStreams.set(taskId, new Set());
    }
    this.taskStreams.get(taskId).add(connectionId);
  }

  // Unsubscribe connection from task
  unsubscribeFromTask(connectionId, taskId) {
    const connection = this.connections.get(connectionId);
    if (connection) {
      connection.taskIds.delete(taskId);
    }

    const taskSubscribers = this.taskStreams.get(taskId);
    if (taskSubscribers) {
      taskSubscribers.delete(connectionId);
    }
  }

  // Broadcast task update to all subscribed connections
  broadcastTaskUpdate(taskId, update) {
    const subscribers = this.taskStreams.get(taskId);
    if (!subscribers) return;

    const message = {
      type: 'task_update',
      taskId,
      timestamp: Date.now(),
      update
    };

    for (const connectionId of subscribers) {
      this.emit('message', connectionId, message);
    }
  }

  // Send progress update
  sendProgress(taskId, progress, details = {}) {
    this.broadcastTaskUpdate(taskId, {
      type: 'progress',
      progress, // 0-100
      details
    });
  }

  // Send status update
  sendStatus(taskId, status, message = '') {
    this.broadcastTaskUpdate(taskId, {
      type: 'status',
      status, // 'running', 'completed', 'failed', etc.
      message
    });
  }

  // Send log message
  sendLog(taskId, level, message) {
    this.broadcastTaskUpdate(taskId, {
      type: 'log',
      level, // 'info', 'warn', 'error', 'debug'
      message
    });
  }

  // Send token usage
  sendTokenUsage(taskId, inputTokens, outputTokens, cost) {
    this.broadcastTaskUpdate(taskId, {
      type: 'tokens',
      inputTokens,
      outputTokens,
      totalTokens: inputTokens + outputTokens,
      cost
    });
  }

  // Send tool execution result
  sendToolResult(taskId, toolName, result, duration) {
    this.broadcastTaskUpdate(taskId, {
      type: 'tool_result',
      toolName,
      result,
      duration
    });
  }

  // Close connection
  closeConnection(connectionId) {
    const connection = this.connections.get(connectionId);
    if (connection) {
      connection.isActive = false;
      for (const taskId of connection.taskIds) {
        this.unsubscribeFromTask(connectionId, taskId);
      }
      this.connections.delete(connectionId);
    }
  }

  // Get active connections count
  getConnectionCount() {
    let count = 0;
    for (const conn of this.connections.values()) {
      if (conn.isActive) count++;
    }
    return count;
  }

  // Get task subscribers
  getTaskSubscribers(taskId) {
    return Array.from(this.taskStreams.get(taskId) || []);
  }

  // Get connection status
  getConnectionStatus(connectionId) {
    const connection = this.connections.get(connectionId);
    if (!connection) return null;

    return {
      id: connectionId,
      type: connection.type,
      isActive: connection.isActive,
      createdAt: connection.createdAt,
      subscribedTasks: Array.from(connection.taskIds),
      uptime: Date.now() - connection.createdAt
    };
  }

  // Get all connections status
  getAllConnectionsStatus() {
    const statuses = [];
    for (const [id, conn] of this.connections.entries()) {
      if (conn.isActive) {
        statuses.push(this.getConnectionStatus(id));
      }
    }
    return statuses;
  }

  // Format message for SSE
  formatSSEMessage(data) {
    const lines = [
      `data: ${JSON.stringify(data)}`
    ];
    return lines.join('\n') + '\n\n';
  }

  // Create SSE stream
  createSSEStream(connectionId) {
    return (req, res) => {
      res.setHeader('Content-Type', 'text/event-stream');
      res.setHeader('Cache-Control', 'no-cache');
      res.setHeader('Connection', 'keep-alive');
      res.setHeader('Access-Control-Allow-Origin', '*');

      this.registerConnection(connectionId, 'sse');

      const heartbeat = setInterval(() => {
        res.write(': heartbeat\n\n');
      }, 30000);

      this.on('message', (conId, message) => {
        if (conId === connectionId) {
          res.write(this.formatSSEMessage(message));
        }
      });

      req.on('close', () => {
        clearInterval(heartbeat);
        this.closeConnection(connectionId);
        res.end();
      });
    };
  }

  // Get streaming stats
  getStreamingStats() {
    return {
      activeConnections: this.getConnectionCount(),
      totalConnections: this.connections.size,
      activeTasks: this.taskStreams.size,
      connections: this.getAllConnectionsStatus()
    };
  }
}

// Export singleton instance
export const streamingManager = new StreamingManager();
