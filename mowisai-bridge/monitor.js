// Agent Performance Monitoring Module
// Tracks metrics, performance, and provides dashboard endpoints

export class PerformanceMonitor {
  constructor() {
    this.metrics = {
      tasksCompleted: 0,
      tasksFailed: 0,
      tasksInProgress: new Map(),
      agentMetrics: {},
      totalTokensUsed: 0,
      totalCostIncurred: 0,
      startTime: Date.now(),
      taskHistory: []
    };
  }

  startTask(taskId, agentType, taskDescription) {
    const startTime = Date.now();
    const taskMetric = {
      taskId,
      agentType,
      taskDescription,
      startTime,
      status: 'in_progress',
      inputTokens: 0,
      outputTokens: 0,
      cost: 0,
      errors: []
    };
    
    this.metrics.tasksInProgress.set(taskId, taskMetric);
    return taskMetric;
  }

  completeTask(taskId, inputTokens = 0, outputTokens = 0, cost = 0) {
    const task = this.metrics.tasksInProgress.get(taskId);
    
    if (task) {
      const endTime = Date.now();
      task.status = 'completed';
      task.endTime = endTime;
      task.duration = endTime - task.startTime;
      task.inputTokens = inputTokens;
      task.outputTokens = outputTokens;
      task.cost = cost;
      
      this.metrics.tasksCompleted++;
      this.metrics.totalTokensUsed += (inputTokens + outputTokens);
      this.metrics.totalCostIncurred += cost;
      this.metrics.taskHistory.push(task);
      
      // Keep only last 1000 tasks
      if (this.metrics.taskHistory.length > 1000) {
        this.metrics.taskHistory.shift();
      }
      
      this.metrics.tasksInProgress.delete(taskId);
      this._updateAgentMetrics(task);
    }
  }

  failTask(taskId, error) {
    const task = this.metrics.tasksInProgress.get(taskId);
    
    if (task) {
      const endTime = Date.now();
      task.status = 'failed';
      task.endTime = endTime;
      task.duration = endTime - task.startTime;
      task.error = error.message || String(error);
      task.errors.push(error);
      
      this.metrics.tasksFailed++;
      this.metrics.taskHistory.push(task);
      
      if (this.metrics.taskHistory.length > 1000) {
        this.metrics.taskHistory.shift();
      }
      
      this.metrics.tasksInProgress.delete(taskId);
      this._updateAgentMetrics(task);
    }
  }

  _updateAgentMetrics(task) {
    const agentType = task.agentType;
    
    if (!this.metrics.agentMetrics[agentType]) {
      this.metrics.agentMetrics[agentType] = {
        tasksCompleted: 0,
        tasksFailed: 0,
        totalDuration: 0,
        averageDuration: 0,
        totalCost: 0,
        totalTokens: 0,
        successRate: 100
      };
    }
    
    const agentMetric = this.metrics.agentMetrics[agentType];
    
    if (task.status === 'completed') {
      agentMetric.tasksCompleted++;
      agentMetric.totalDuration += task.duration;
      agentMetric.averageDuration = agentMetric.totalDuration / agentMetric.tasksCompleted;
      agentMetric.totalCost += task.cost;
      agentMetric.totalTokens += (task.inputTokens + task.outputTokens);
    } else if (task.status === 'failed') {
      agentMetric.tasksFailed++;
    }
    
    const totalTasks = agentMetric.tasksCompleted + agentMetric.tasksFailed;
    agentMetric.successRate = totalTasks > 0 
      ? (agentMetric.tasksCompleted / totalTasks) * 100 
      : 100;
  }

  getOverallStats() {
    return {
      tasksCompleted: this.metrics.tasksCompleted,
      tasksFailed: this.metrics.tasksFailed,
      tasksInProgress: this.metrics.tasksInProgress.size,
      totalTokensUsed: this.metrics.totalTokensUsed,
      totalCostIncurred: parseFloat(this.metrics.totalCostIncurred.toFixed(4)),
      uptime: Date.now() - this.metrics.startTime,
      successRate: this.metrics.tasksCompleted + this.metrics.tasksFailed > 0
        ? (this.metrics.tasksCompleted / (this.metrics.tasksCompleted + this.metrics.tasksFailed)) * 100
        : 0,
      agentMetrics: this.metrics.agentMetrics
    };
  }

  getAgentStats(agentType) {
    return this.metrics.agentMetrics[agentType] || null;
  }

  getTaskHistory(limit = 50) {
    return this.metrics.taskHistory.slice(-limit);
  }

  getInProgressTasks() {
    return Array.from(this.metrics.tasksInProgress.values());
  }

  getDashboardData() {
    const stats = this.getOverallStats();
    return {
      summary: {
        completed: stats.tasksCompleted,
        failed: stats.tasksFailed,
        inProgress: stats.tasksInProgress,
        successRate: parseFloat(stats.successRate.toFixed(2)),
        totalCost: stats.totalCostIncurred,
        totalTokens: stats.totalTokensUsed,
        uptime: stats.uptime
      },
      agents: stats.agentMetrics,
      recentTasks: this.getTaskHistory(10),
      inProgress: this.getInProgressTasks()
    };
  }

  reset() {
    this.metrics = {
      tasksCompleted: 0,
      tasksFailed: 0,
      tasksInProgress: new Map(),
      agentMetrics: {},
      totalTokensUsed: 0,
      totalCostIncurred: 0,
      startTime: Date.now(),
      taskHistory: []
    };
  }
}

// Export singleton instance
export const monitor = new PerformanceMonitor();
