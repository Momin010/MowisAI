// Configuration module for MowisAI Engine
// Manages all agent settings, timeouts, resource limits, etc.

export class Config {
  constructor() {
    // Agent timeout configuration (in seconds)
    this.agentTimeouts = {
      default: 30,
      coder: 60,
      data_scientist: 120,
      financial_analyst: 45,
      researcher: 90,
      designer: 75,
      writer: 45
    };

    // Resource limits configuration
    this.resourceLimits = {
      memory_mb: parseInt(process.env.AGENT_MEMORY_MB || '512'),
      cpu_percent: parseInt(process.env.AGENT_CPU_PERCENT || '50'),
      max_tasks_concurrent: parseInt(process.env.MAX_CONCURRENT_TASKS || '5'),
      max_task_history: parseInt(process.env.MAX_TASK_HISTORY || '1000')
    };

    // Cost configuration for different models
    this.costPerToken = {
      'llama-3.3-70b-versatile': { input: 0.59 / 1000000, output: 0.79 / 1000000 },
      'gpt-4': { input: 0.03 / 1000, output: 0.06 / 1000 },
      'gpt-3.5-turbo': { input: 0.0005 / 1000, output: 0.0015 / 1000 },
      'claude-3-opus': { input: 0.015 / 1000, output: 0.075 / 1000 }
    };

    // Webhook configuration
    this.webhooks = {
      enabled: process.env.WEBHOOKS_ENABLED === 'true',
      endpoints: {
        slack: process.env.SLACK_WEBHOOK_URL || '',
        email: process.env.EMAIL_WEBHOOK_URL || '',
        custom: process.env.CUSTOM_WEBHOOK_URL || ''
      },
      retryAttempts: 3,
      retryDelayMs: 1000
    };

    // Model configuration
    this.models = {
      default: process.env.DEFAULT_MODEL || 'llama-3.3-70b-versatile',
      available: ['llama-3.3-70b-versatile', 'gpt-4', 'gpt-3.5-turbo', 'claude-3-opus'],
      perAgentType: {
        coder: 'gpt-4',
        data_scientist: 'claude-3-opus',
        financial_analyst: 'gpt-4',
        researcher: 'llama-3.3-70b-versatile',
        designer: 'gpt-4',
        writer: 'claude-3-opus'
      }
    };

    // Database configuration for task history
    this.database = {
      enabled: process.env.DB_ENABLED === 'true',
      type: process.env.DB_TYPE || 'sqlite',
      path: process.env.DB_PATH || './mowisai_tasks.db',
      url: process.env.DATABASE_URL || '',
      retentionDays: parseInt(process.env.TASK_RETENTION_DAYS || '90')
    };

    // Streaming configuration (WebSocket/SSE)
    this.streaming = {
      enabled: process.env.STREAMING_ENABLED === 'true',
      protocol: process.env.STREAMING_PROTOCOL || 'sse',
      updateIntervalMs: parseInt(process.env.STREAM_UPDATE_INTERVAL || '500'),
      maxConnections: parseInt(process.env.MAX_STREAM_CONNECTIONS || '100')
    };

    // Sandbox resource limits
    this.sandbox = {
      memoryMb: {
        min: 256,
        default: 512,
        max: 8192
      },
      cpuPercent: {
        min: 10,
        default: 50,
        max: 100
      },
      diskMb: {
        default: 5000,
        max: 50000
      },
      networkBandwidthMbps: {
        default: 100,
        max: 1000
      }
    };

    // Agent collaboration settings
    this.collaboration = {
      enabled: true,
      maxHelpRequests: 3,
      helpTimeoutSeconds: 60,
      priorityLevels: ['low', 'normal', 'high', 'critical']
    };

    // Prompt versioning
    this.prompts = {
      versioningEnabled: true,
      storageDir: process.env.PROMPT_STORAGE_DIR || './prompts',
      maxVersions: 10
    };
  }

  getAgentTimeout(agentType) {
    return this.agentTimeouts[agentType] || this.agentTimeouts.default;
  }

  setAgentTimeout(agentType, timeoutSeconds) {
    this.agentTimeouts[agentType] = timeoutSeconds;
  }

  getResourceLimits() {
    return { ...this.resourceLimits };
  }

  setResourceLimits(memory, cpu, maxConcurrent) {
    if (memory) this.resourceLimits.memory_mb = memory;
    if (cpu) this.resourceLimits.cpu_percent = cpu;
    if (maxConcurrent) this.resourceLimits.max_tasks_concurrent = maxConcurrent;
  }

  getModelForAgent(agentType) {
    return this.models.perAgentType[agentType] || this.models.default;
  }

  setDefaultModel(model) {
    if (this.models.available.includes(model)) {
      this.models.default = model;
    }
  }

  estimateCost(modelName, inputTokens, outputTokens) {
    const rate = this.costPerToken[modelName];
    if (!rate) return null;
    return (inputTokens * rate.input) + (outputTokens * rate.output);
  }

  isWebhookEnabled() {
    return this.webhooks.enabled;
  }

  getWebhookUrl(type) {
    return this.webhooks.endpoints[type] || null;
  }

  isStreamingEnabled() {
    return this.streaming.enabled;
  }

  isDatabaseEnabled() {
    return this.database.enabled;
  }

  toJSON() {
    return {
      agentTimeouts: this.agentTimeouts,
      resourceLimits: this.resourceLimits,
      models: this.models,
      database: this.database,
      streaming: this.streaming,
      sandbox: this.sandbox,
      collaboration: this.collaboration,
      prompts: this.prompts
    };
  }
}

// Export singleton instance
export const config = new Config();
