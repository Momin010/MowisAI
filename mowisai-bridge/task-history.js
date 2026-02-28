// Task History & Replay Module
// Stores and retrieves task execution history

export class TaskHistoryManager {
  constructor() {
    this.tasks = [];
    this.taskIndex = new Map(); // taskId -> task
    this.maxTasks = 10000;
  }

  // Record a new task
  recordTask(taskId, input) {
    const task = {
      taskId,
      input,
      createdAt: new Date().toISOString(),
      startedAt: null,
      completedAt: null,
      status: 'pending',
      agentType: null,
      steps: [],
      outputs: [],
      totalCost: 0,
      totalTokens: 0,
      errors: [],
      metadata: {}
    };

    this.tasks.push(task);
    this.taskIndex.set(taskId, task);

    // Maintain size limit
    if (this.tasks.length > this.maxTasks) {
      const removed = this.tasks.shift();
      this.taskIndex.delete(removed.taskId);
    }

    return task;
  }

  // Start task execution
  startTask(taskId, agentType) {
    const task = this.taskIndex.get(taskId);
    if (!task) throw new Error(`Task ${taskId} not found`);

    task.status = 'running';
    task.startedAt = new Date().toISOString();
    task.agentType = agentType;
  }

  // Add a step to task
  addStep(taskId, stepName, stepData, duration = 0) {
    const task = this.taskIndex.get(taskId);
    if (!task) throw new Error(`Task ${taskId} not found`);

    const step = {
      name: stepName,
      timestamp: new Date().toISOString(),
      data: stepData,
      duration
    };

    task.steps.push(step);
  }

  // Record task output
  recordOutput(taskId, output) {
    const task = this.taskIndex.get(taskId);
    if (!task) throw new Error(`Task ${taskId} not found`);

    task.outputs.push({
      timestamp: new Date().toISOString(),
      content: output
    });
  }

  // Record token usage
  recordTokenUsage(taskId, inputTokens, outputTokens, cost) {
    const task = this.taskIndex.get(taskId);
    if (!task) throw new Error(`Task ${taskId} not found`);

    task.totalTokens += (inputTokens + outputTokens);
    task.totalCost += cost;
  }

  // Record error
  recordError(taskId, error) {
    const task = this.taskIndex.get(taskId);
    if (!task) throw new Error(`Task ${taskId} not found`);

    task.errors.push({
      timestamp: new Date().toISOString(),
      message: error.message || String(error),
      stack: error.stack || null
    });
  }

  // Complete task
  completeTask(taskId, status = 'completed') {
    const task = this.taskIndex.get(taskId);
    if (!task) throw new Error(`Task ${taskId} not found`);

    task.status = status; // 'completed', 'failed', 'cancelled'
    task.completedAt = new Date().toISOString();
  }

  // Get task by ID
  getTask(taskId) {
    return this.taskIndex.get(taskId) || null;
  }

  // Get all tasks
  getAllTasks(limit = 100) {
    return this.tasks.slice(-limit);
  }

  // Search tasks
  searchTasks(query) {
    const results = [];
    
    for (const task of this.tasks) {
      if (
        task.taskId.includes(query) ||
        task.input.includes(query) ||
        task.agentType?.includes(query) ||
        task.outputs.some(o => o.content.includes(query))
      ) {
        results.push(task);
      }
    }

    return results;
  }

  // Get tasks by status
  getTasksByStatus(status) {
    return this.tasks.filter(t => t.status === status);
  }

  // Get tasks by agent type
  getTasksByAgent(agentType) {
    return this.tasks.filter(t => t.agentType === agentType);
  }

  // Get task statistics
  getTaskStats() {
    const stats = {
      totalTasks: this.tasks.length,
      byStatus: {},
      byAgent: {},
      totalTokens: 0,
      totalCost: 0,
      averageCostPerTask: 0,
      averageTokensPerTask: 0,
      recentTasks: []
    };

    this.tasks.forEach(task => {
      stats.byStatus[task.status] = (stats.byStatus[task.status] || 0) + 1;
      if (task.agentType) {
        stats.byAgent[task.agentType] = (stats.byAgent[task.agentType] || 0) + 1;
      }
      stats.totalTokens += task.totalTokens;
      stats.totalCost += task.totalCost;
    });

    if (this.tasks.length > 0) {
      stats.averageCostPerTask = parseFloat((stats.totalCost / this.tasks.length).toFixed(4));
      stats.averageTokensPerTask = Math.floor(stats.totalTokens / this.tasks.length);
    }

    stats.recentTasks = this.tasks.slice(-10).map(t => ({
      taskId: t.taskId,
      status: t.status,
      agentType: t.agentType,
      createdAt: t.createdAt,
      cost: t.totalCost,
      tokens: t.totalTokens
    }));

    return stats;
  }

  // Export task as JSON (for replay)
  exportTask(taskId) {
    const task = this.taskIndex.get(taskId);
    if (!task) throw new Error(`Task ${taskId} not found`);

    return JSON.stringify(task, null, 2);
  }

  // Import/Replay a task
  replayTask(taskJson) {
    const taskData = typeof taskJson === 'string' ? JSON.parse(taskJson) : taskJson;
    
    const replayId = `replay-${taskData.taskId}-${Date.now()}`;
    const replayed = {
      ...taskData,
      taskId: replayId,
      createdAt: new Date().toISOString(),
      startedAt: null,
      completedAt: null,
      status: 'pending',
      steps: [],
      outputs: [],
      totalCost: 0,
      totalTokens: 0,
      errors: [],
      isReplayed: true,
      originTaskId: taskData.taskId
    };

    this.tasks.push(replayed);
    this.taskIndex.set(replayId, replayed);

    if (this.tasks.length > this.maxTasks) {
      const removed = this.tasks.shift();
      this.taskIndex.delete(removed.taskId);
    }

    return replayed;
  }

  // Clear history
  clearHistory() {
    this.tasks = [];
    this.taskIndex.clear();
  }

  // Clean old tasks (older than days)
  cleanOldTasks(days = 90) {
    const cutoffTime = Date.now() - (days * 24 * 60 * 60 * 1000);
    
    this.tasks = this.tasks.filter(task => {
      const taskTime = new Date(task.createdAt).getTime();
      if (taskTime < cutoffTime) {
        this.taskIndex.delete(task.taskId);
        return false;
      }
      return true;
    });
  }

  // Get task timeline
  getTaskTimeline(taskId) {
    const task = this.taskIndex.get(taskId);
    if (!task) return null;

    const events = [];
    events.push({ 
      type: 'created', 
      timestamp: task.createdAt,
      details: { input: task.input }
    });

    if (task.startedAt) {
      events.push({
        type: 'started',
        timestamp: task.startedAt,
        details: { agentType: task.agentType }
      });
    }

    task.steps.forEach(step => {
      events.push({
        type: 'step',
        timestamp: step.timestamp,
        details: { name: step.name, duration: step.duration }
      });
    });

    task.errors.forEach(error => {
      events.push({
        type: 'error',
        timestamp: error.timestamp,
        details: { message: error.message }
      });
    });

    if (task.completedAt) {
      events.push({
        type: 'completed',
        timestamp: task.completedAt,
        details: { status: task.status, totalCost: task.totalCost }
      });
    }

    return { taskId, events };
  }
}

// Export singleton instance
export const taskHistory = new TaskHistoryManager();
