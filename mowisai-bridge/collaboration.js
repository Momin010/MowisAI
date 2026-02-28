// Agent Collaboration Module
// Allows agents to request help from other agents mid-task

export class CollaborationManager {
  constructor() {
    this.helpRequests = new Map();
    this.requestHistory = [];
    this.agentCapabilities = new Map();
    this.maxHelpRequests = 3;
    this.helpTimeoutSeconds = 60;
  }

  // Register agent capabilities
  registerAgentCapabilities(agentId, capabilities) {
    this.agentCapabilities.set(agentId, {
      agentId,
      capabilities,
      registeredAt: new Date().toISOString()
    });
  }

  // Request help from another agent
  async requestHelp(fromAgentId, toAgentType, task, priority = 'normal') {
    const requestId = `help-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;

    const helpRequest = {
      requestId,
      fromAgent: fromAgentId,
      requestedAgentType: toAgentType,
      task,
      priority,
      status: 'pending',
      createdAt: new Date().toISOString(),
      deadline: new Date(Date.now() + this.helpTimeoutSeconds * 1000).toISOString(),
      result: null,
      error: null
    };

    this.helpRequests.set(requestId, helpRequest);
    this.requestHistory.push(helpRequest);

    // Emit event for help request
    this.emit('help_requested', helpRequest);

    return requestId;
  }

  // Provide help/response to request
  respondToHelpRequest(requestId, result) {
    const helpRequest = this.helpRequests.get(requestId);
    if (!helpRequest) {
      throw new Error(`Help request ${requestId} not found`);
    }

    helpRequest.status = 'completed';
    helpRequest.result = result;
    helpRequest.completedAt = new Date().toISOString();

    this.emit('help_provided', helpRequest);

    return helpRequest;
  }

  // Mark help request as failed
  failHelpRequest(requestId, error) {
    const helpRequest = this.helpRequests.get(requestId);
    if (!helpRequest) {
      throw new Error(`Help request ${requestId} not found`);
    }

    helpRequest.status = 'failed';
    helpRequest.error = error.message || String(error);
    helpRequest.failedAt = new Date().toISOString();

    this.emit('help_failed', helpRequest);

    return helpRequest;
  }

  // Get help request status
  getHelpRequestStatus(requestId) {
    const helpRequest = this.helpRequests.get(requestId);
    if (!helpRequest) return null;

    return {
      requestId,
      status: helpRequest.status,
      fromAgent: helpRequest.fromAgent,
      requestedAgentType: helpRequest.requestedAgentType,
      priority: helpRequest.priority,
      createdAt: helpRequest.createdAt,
      result: helpRequest.result,
      error: helpRequest.error
    };
  }

  // Wait for help request completion
  async waitForHelp(requestId, timeoutMs = 60000) {
    const startTime = Date.now();

    return new Promise((resolve, reject) => {
      const check = () => {
        const helpRequest = this.helpRequests.get(requestId);

        if (!helpRequest) {
          reject(new Error(`Help request ${requestId} not found`));
          return;
        }

        if (helpRequest.status !== 'pending') {
          if (helpRequest.status === 'completed') {
            resolve(helpRequest.result);
          } else {
            reject(new Error(helpRequest.error || 'Help request failed'));
          }
          return;
        }

        if (Date.now() - startTime > timeoutMs) {
          this.failHelpRequest(requestId, new Error('Help request timeout'));
          reject(new Error('Help request timeout'));
          return;
        }

        setTimeout(check, 100);
      };

      check();
    });
  }

  // Get pending help requests
  getPendingHelpRequests() {
    const pending = [];
    for (const [id, request] of this.helpRequests.entries()) {
      if (request.status === 'pending') {
        pending.push(request);
      }
    }
    return pending;
  }

  // Get help requests for agent type
  getHelpRequestsForAgentType(agentType) {
    const requests = [];
    for (const [id, request] of this.helpRequests.entries()) {
      if (request.requestedAgentType === agentType && request.status === 'pending') {
        requests.push(request);
      }
    }
    // Sort by priority and created time
    return requests.sort((a, b) => {
      const priorityOrder = { critical: 0, high: 1, normal: 2, low: 3 };
      const diff = priorityOrder[a.priority] - priorityOrder[b.priority];
      return diff !== 0 ? diff : new Date(a.createdAt) - new Date(b.createdAt);
    });
  }

  // Get help from specific agent type
  async getHelpFromAgent(fromAgentId, toAgentType, task, priority = 'normal') {
    const requestId = await this.requestHelp(fromAgentId, toAgentType, task, priority);

    try {
      const result = await this.waitForHelp(requestId);
      return result;
    } catch (error) {
      throw error;
    }
  }

  // Find best agent for help
  findBestAgentForTask(requiredCapabilities) {
    let bestAgent = null;
    let bestScore = 0;

    for (const [agentId, capabilityInfo] of this.agentCapabilities.entries()) {
      let score = 0;
      for (const capability of requiredCapabilities) {
        if (capabilityInfo.capabilities.includes(capability)) {
          score++;
        }
      }

      if (score > bestScore) {
        bestScore = score;
        bestAgent = agentId;
      }
    }

    return { agentId: bestAgent, matchScore: bestScore / requiredCapabilities.length };
  }

  // Get collaboration history
  getCollaborationHistory(agentId = null, limit = 50) {
    let history = this.requestHistory;

    if (agentId) {
      history = history.filter(r => 
        r.fromAgent === agentId || r.completedBy === agentId
      );
    }

    return history.slice(-limit);
  }

  // Get collaboration statistics
  getCollaborationStats() {
    const stats = {
      totalRequests: this.requestHistory.length,
      pendingRequests: this.getPendingHelpRequests().length,
      completedRequests: 0,
      failedRequests: 0,
      successRate: 0,
      averageCompletionTime: 0,
      byAgentType: {},
      byPriority: {}
    };

    let totalCompletionTime = 0;
    let completionCount = 0;

    for (const request of this.requestHistory) {
      if (request.status === 'completed') {
        stats.completedRequests++;
        stats.byAgentType[request.requestedAgentType] = 
          (stats.byAgentType[request.requestedAgentType] || 0) + 1;

        if (request.completedAt) {
          const time = new Date(request.completedAt) - new Date(request.createdAt);
          totalCompletionTime += time;
          completionCount++;
        }
      } else if (request.status === 'failed') {
        stats.failedRequests++;
      }

      stats.byPriority[request.priority] = (stats.byPriority[request.priority] || 0) + 1;
    }

    if (this.requestHistory.length > 0) {
      stats.successRate = (stats.completedRequests / this.requestHistory.length) * 100;
    }

    if (completionCount > 0) {
      stats.averageCompletionTime = Math.floor(totalCompletionTime / completionCount);
    }

    return stats;
  }

  // Clear old requests
  clearOldRequests(olderThanMs = 86400000) { // 24 hours default
    const cutoff = Date.now() - olderThanMs;
    const toDelete = [];

    for (const [id, request] of this.helpRequests.entries()) {
      if (new Date(request.createdAt).getTime() < cutoff) {
        toDelete.push(id);
      }
    }

    for (const id of toDelete) {
      this.helpRequests.delete(id);
    }

    return toDelete.length;
  }

  // Export collaboration data
  exportCollaborationData() {
    return {
      requests: Array.from(this.helpRequests.values()),
      history: this.requestHistory,
      agentCapabilities: Array.from(this.agentCapabilities.values()),
      stats: this.getCollaborationStats()
    };
  }
}

// Add event emitter capability
import EventEmitter from 'events';

CollaborationManager.prototype = Object.assign(
  CollaborationManager.prototype,
  EventEmitter.prototype
);

EventEmitter.call(CollaborationManager.prototype);

// Export singleton instance
export const collaborationManager = new CollaborationManager();
