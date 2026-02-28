// Webhook Integration Module
// Handles webhooks for Slack, email, and custom endpoints

export class WebhookManager {
  constructor() {
    this.webhooks = new Map();
    this.webhookHistory = [];
    this.retryQueue = [];
    this.maxRetries = 3;
    this.retryDelayMs = 1000;
  }

  // Register a webhook
  registerWebhook(webhookId, config) {
    const webhook = {
      id: webhookId,
      url: config.url,
      type: config.type, // 'slack', 'email', 'custom'
      events: config.events || ['task_completed', 'task_failed', 'task_started'],
      active: true,
      headers: config.headers || {},
      retryOnFailure: config.retryOnFailure !== false,
      createdAt: new Date().toISOString()
    };

    this.webhooks.set(webhookId, webhook);
    return webhook;
  }

  // Unregister a webhook
  unregisterWebhook(webhookId) {
    return this.webhooks.delete(webhookId);
  }

  // Get webhook
  getWebhook(webhookId) {
    return this.webhooks.get(webhookId) || null;
  }

  // Get all webhooks
  getAllWebhooks() {
    return Array.from(this.webhooks.values());
  }

  // Enable webhook
  enableWebhook(webhookId) {
    const webhook = this.webhooks.get(webhookId);
    if (webhook) {
      webhook.active = true;
    }
  }

  // Disable webhook
  disableWebhook(webhookId) {
    const webhook = this.webhooks.get(webhookId);
    if (webhook) {
      webhook.active = false;
    }
  }

  // Send webhook event
  async sendWebhookEvent(event, data) {
    const promises = [];

    for (const [id, webhook] of this.webhooks.entries()) {
      if (webhook.active && webhook.events.includes(event)) {
        promises.push(this._sendWebhook(webhook, event, data));
      }
    }

    return Promise.all(promises);
  }

  // Send individual webhook with retry
  async _sendWebhook(webhook, event, data, retryCount = 0) {
    const payload = {
      event,
      timestamp: new Date().toISOString(),
      data
    };

    const webhookEvent = {
      webhookId: webhook.id,
      event,
      status: 'pending',
      payload,
      attempt: retryCount + 1,
      createdAt: new Date().toISOString()
    };

    try {
      const response = await fetch(webhook.url, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          ...webhook.headers
        },
        body: JSON.stringify(payload),
        timeout: 10000
      });

      if (response.ok) {
        webhookEvent.status = 'success';
        webhookEvent.response = response.status;
      } else {
        throw new Error(`HTTP ${response.status}`);
      }
    } catch (error) {
      webhookEvent.status = 'failed';
      webhookEvent.error = error.message;

      if (webhook.retryOnFailure && retryCount < this.maxRetries) {
        const delay = this.retryDelayMs * Math.pow(2, retryCount);
        this.retryQueue.push({
          webhook,
          event,
          data,
          retryCount: retryCount + 1,
          delayMs: delay
        });
      }
    }

    this.webhookHistory.push(webhookEvent);
    if (this.webhookHistory.length > 1000) {
      this.webhookHistory.shift();
    }

    return webhookEvent;
  }

  // Process retry queue
  async processRetryQueue() {
    if (this.retryQueue.length === 0) return;

    const now = Date.now();
    const toProcess = [];
    const remaining = [];

    for (const item of this.retryQueue) {
      if (item.nextRetry === undefined) {
        item.nextRetry = now + item.delayMs;
      }

      if (item.nextRetry <= now) {
        toProcess.push(item);
      } else {
        remaining.push(item);
      }
    }

    this.retryQueue = remaining;

    for (const item of toProcess) {
      await this._sendWebhook(item.webhook, item.event, item.data, item.retryCount);
    }
  }

  // Format Slack message
  formatSlackMessage(event, data) {
    const colors = {
      task_completed: '#36a64f',
      task_failed: '#ff0000',
      task_started: '#0099ff'
    };

    return {
      attachments: [{
        fallback: `Task ${event}`,
        color: colors[event] || '#999999',
        title: event.replace(/_/g, ' ').toUpperCase(),
        text: JSON.stringify(data, null, 2),
        ts: Math.floor(Date.now() / 1000)
      }]
    };
  }

  // Send Slack notification
  async sendSlackNotification(webhookId, event, data) {
    const webhook = this.webhooks.get(webhookId);
    if (!webhook || webhook.type !== 'slack') {
      throw new Error('Invalid Slack webhook');
    }

    const message = this.formatSlackMessage(event, data);
    return this._sendWebhook(webhook, event, message);
  }

  // Send email notification
  async sendEmailNotification(webhookId, event, data) {
    const webhook = this.webhooks.get(webhookId);
    if (!webhook || webhook.type !== 'email') {
      throw new Error('Invalid email webhook');
    }

    const emailPayload = {
      subject: `MowisAI Task: ${event.replace(/_/g, ' ')}`,
      body: JSON.stringify(data, null, 2),
      timestamp: new Date().toISOString()
    };

    return this._sendWebhook(webhook, event, emailPayload);
  }

  // Get webhook history
  getWebhookHistory(limit = 100) {
    return this.webhookHistory.slice(-limit);
  }

  // Get webhook statistics
  getWebhookStats() {
    const stats = {
      totalWebhooks: this.webhooks.size,
      activeWebhooks: 0,
      webhookHistory: this.webhookHistory.length,
      byType: {},
      byStatus: {}
    };

    for (const webhook of this.webhooks.values()) {
      if (webhook.active) stats.activeWebhooks++;
      stats.byType[webhook.type] = (stats.byType[webhook.type] || 0) + 1;
    }

    for (const event of this.webhookHistory) {
      stats.byStatus[event.status] = (stats.byStatus[event.status] || 0) + 1;
    }

    return stats;
  }

  // Test webhook
  async testWebhook(webhookId) {
    const webhook = this.webhooks.get(webhookId);
    if (!webhook) throw new Error('Webhook not found');

    const testData = {
      taskId: 'test-' + Date.now(),
      message: 'Test webhook notification',
      timestamp: new Date().toISOString()
    };

    return this._sendWebhook(webhook, 'test_event', testData);
  }

  // Clear history
  clearHistory() {
    this.webhookHistory = [];
  }

  // Export webhooks configuration
  exportWebhooks() {
    const config = [];
    for (const webhook of this.webhooks.values()) {
      config.push({
        id: webhook.id,
        url: webhook.url,
        type: webhook.type,
        events: webhook.events,
        active: webhook.active
      });
    }
    return config;
  }
}

// Export singleton instance
export const webhookManager = new WebhookManager();
