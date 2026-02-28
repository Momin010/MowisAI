// API Endpoints Module
// REST API endpoints for all features

import express from 'express';
import { config } from './config.js';
import { monitor } from './monitor.js';
import { costEstimator } from './cost-estimator.js';
import { errorHandler } from './error-handler.js';
import { streamingManager } from './streaming.js';
import { taskHistory } from './task-history.js';
import { agentRegistry } from './agent-registry.js';
import { webhookManager } from './webhooks.js';
import { promptVersionManager } from './prompt-versioning.js';
import { collaborationManager } from './collaboration.js';

export function createAPIRouter() {
  const router = express.Router();

  // ============ Configuration Endpoints ============
  router.get('/api/config', (req, res) => {
    res.json(config.toJSON());
  });

  router.post('/api/config/timeout/:agentType', (req, res) => {
    const { agentType } = req.params;
    const { timeout } = req.body;
    config.setAgentTimeout(agentType, timeout);
    res.json({ success: true, agentType, timeout });
  });

  router.post('/api/config/resources', (req, res) => {
    const { memory, cpu, maxConcurrent } = req.body;
    config.setResourceLimits(memory, cpu, maxConcurrent);
    res.json({ success: true, limits: config.getResourceLimits() });
  });

  router.post('/api/config/model', (req, res) => {
    const { model } = req.body;
    config.setDefaultModel(model);
    res.json({ success: true, model: config.models.default });
  });

  // ============ Monitoring/Performance Endpoints ============
  router.get('/api/monitor/stats', (req, res) => {
    res.json(monitor.getOverallStats());
  });

  router.get('/api/monitor/dashboard', (req, res) => {
    res.json(monitor.getDashboardData());
  });

  router.get('/api/monitor/agent/:agentType', (req, res) => {
    const { agentType } = req.params;
    const stats = monitor.getAgentStats(agentType);
    res.json(stats || { error: 'Agent not found' });
  });

  router.get('/api/monitor/history', (req, res) => {
    const { limit = 50 } = req.query;
    res.json(monitor.getTaskHistory(parseInt(limit)));
  });

  router.get('/api/monitor/in-progress', (req, res) => {
    res.json(monitor.getInProgressTasks());
  });

  // ============ Cost Estimation Endpoints ============
  router.post('/api/cost/estimate-message', (req, res) => {
    const { model, message } = req.body;
    const estimation = costEstimator.estimateMessageCost(model, message);
    res.json(estimation);
  });

  router.post('/api/cost/estimate-conversation', (req, res) => {
    const { model, messages } = req.body;
    const estimation = costEstimator.estimateConversationCost(model, messages);
    res.json(estimation);
  });

  router.post('/api/cost/record-usage', (req, res) => {
    const { model, inputTokens, outputTokens } = req.body;
    const usage = costEstimator.recordActualUsage(model, inputTokens, outputTokens);
    res.json(usage);
  });

  router.get('/api/cost/summary', (req, res) => {
    res.json(costEstimator.getCostSummary());
  });

  // ============ Error Handling Endpoints ============
  router.get('/api/errors/summary', (req, res) => {
    const { limit = 20 } = req.query;
    res.json(errorHandler.getErrorSummary(parseInt(limit)));
  });

  router.get('/api/errors/stats', (req, res) => {
    res.json(errorHandler.getErrorStats());
  });

  router.post('/api/errors/clear', (req, res) => {
    errorHandler.clearErrors();
    res.json({ success: true });
  });

  // ============ Streaming Endpoints ============
  router.get('/api/stream/stats', (req, res) => {
    res.json(streamingManager.getStreamingStats());
  });

  router.post('/api/stream/subscribe', (req, res) => {
    const { connectionId, taskId } = req.body;
    try {
      streamingManager.subscribeToTask(connectionId, taskId);
      res.json({ success: true, connectionId, taskId });
    } catch (error) {
      res.status(400).json({ error: error.message });
    }
  });

  router.post('/api/stream/unsubscribe', (req, res) => {
    const { connectionId, taskId } = req.body;
    streamingManager.unsubscribeFromTask(connectionId, taskId);
    res.json({ success: true });
  });

  // ============ Task History Endpoints ============
  router.get('/api/tasks/:taskId', (req, res) => {
    const { taskId } = req.params;
    const task = taskHistory.getTask(taskId);
    res.json(task || { error: 'Task not found' });
  });

  router.get('/api/tasks', (req, res) => {
    const { limit = 50 } = req.query;
    res.json(taskHistory.getAllTasks(parseInt(limit)));
  });

  router.get('/api/tasks/search/:query', (req, res) => {
    const { query } = req.params;
    res.json(taskHistory.searchTasks(query));
  });

  router.get('/api/tasks/status/:status', (req, res) => {
    const { status } = req.params;
    res.json(taskHistory.getTasksByStatus(status));
  });

  router.get('/api/tasks/timeline/:taskId', (req, res) => {
    const { taskId } = req.params;
    const timeline = taskHistory.getTaskTimeline(taskId);
    res.json(timeline || { error: 'Task not found' });
  });

  router.get('/api/tasks/stats', (req, res) => {
    res.json(taskHistory.getTaskStats());
  });

  router.post('/api/tasks/replay', (req, res) => {
    const { taskJson } = req.body;
    try {
      const replayed = taskHistory.replayTask(taskJson);
      res.json({ success: true, replayedTaskId: replayed.taskId });
    } catch (error) {
      res.status(400).json({ error: error.message });
    }
  });

  // ============ Agent Registry Endpoints ============
  router.get('/api/agents', (req, res) => {
    res.json(agentRegistry.getAllAgentTypes());
  });

  router.get('/api/agents/:agentId', (req, res) => {
    const { agentId } = req.params;
    const agent = agentRegistry.getAgentType(agentId);
    res.json(agent || { error: 'Agent not found' });
  });

  router.post('/api/agents/register', (req, res) => {
    const { agentId, config: agentConfig } = req.body;
    try {
      const registered = agentRegistry.registerAgentType(agentId, agentConfig);
      res.json({ success: true, agent: registered });
    } catch (error) {
      res.status(400).json({ error: error.message });
    }
  });

  router.get('/api/agents/registry/stats', (req, res) => {
    res.json(agentRegistry.getRegistryStats());
  });

  // ============ Webhook Endpoints ============
  router.get('/api/webhooks', (req, res) => {
    res.json(webhookManager.getAllWebhooks());
  });

  router.post('/api/webhooks/register', (req, res) => {
    const { webhookId, config: webhookConfig } = req.body;
    try {
      const webhook = webhookManager.registerWebhook(webhookId, webhookConfig);
      res.json({ success: true, webhook });
    } catch (error) {
      res.status(400).json({ error: error.message });
    }
  });

  router.post('/api/webhooks/:webhookId/test', (req, res) => {
    const { webhookId } = req.params;
    webhookManager.testWebhook(webhookId)
      .then(result => res.json(result))
      .catch(error => res.status(400).json({ error: error.message }));
  });

  router.get('/api/webhooks/stats', (req, res) => {
    res.json(webhookManager.getWebhookStats());
  });

  router.get('/api/webhooks/history', (req, res) => {
    const { limit = 50 } = req.query;
    res.json(webhookManager.getWebhookHistory(parseInt(limit)));
  });

  // ============ Prompt Versioning Endpoints ============
  router.get('/api/prompts', (req, res) => {
    res.json(promptVersionManager.getAllPrompts());
  });

  router.post('/api/prompts', (req, res) => {
    const { promptId, content, metadata } = req.body;
    try {
      const prompt = promptVersionManager.createPrompt(promptId, content, metadata);
      res.json({ success: true, prompt });
    } catch (error) {
      res.status(400).json({ error: error.message });
    }
  });

  router.get('/api/prompts/:promptId', (req, res) => {
    const { promptId } = req.params;
    const prompt = promptVersionManager.getPromptContent(promptId);
    res.json({ promptId, content: prompt });
  });

  router.get('/api/prompts/:promptId/versions', (req, res) => {
    const { promptId } = req.params;
    const versions = promptVersionManager.getVersionHistory(promptId);
    res.json(versions);
  });

  router.post('/api/prompts/:promptId/versions', (req, res) => {
    const { promptId } = req.params;
    const { content, message, author } = req.body;
    try {
      const version = promptVersionManager.createVersion(promptId, content, message, author);
      res.json({ success: true, version });
    } catch (error) {
      res.status(400).json({ error: error.message });
    }
  });

  router.post('/api/prompts/:promptId/revert', (req, res) => {
    const { promptId } = req.params;
    const { versionNumber, author } = req.body;
    try {
      const version = promptVersionManager.revertToVersion(promptId, versionNumber, author);
      res.json({ success: true, version });
    } catch (error) {
      res.status(400).json({ error: error.message });
    }
  });

  router.get('/api/prompts/stats', (req, res) => {
    res.json(promptVersionManager.getStats());
  });

  // ============ Collaboration Endpoints ============
  router.get('/api/collaboration/pending', (req, res) => {
    res.json(collaborationManager.getPendingHelpRequests());
  });

  router.post('/api/collaboration/request-help', (req, res) => {
    const { fromAgent, agentType, task, priority } = req.body;
    collaborationManager.requestHelp(fromAgent, agentType, task, priority)
      .then(requestId => res.json({ success: true, requestId }))
      .catch(error => res.status(400).json({ error: error.message }));
  });

  router.post('/api/collaboration/respond', (req, res) => {
    const { requestId, result } = req.body;
    try {
      const response = collaborationManager.respondToHelpRequest(requestId, result);
      res.json({ success: true, response });
    } catch (error) {
      res.status(400).json({ error: error.message });
    }
  });

  router.get('/api/collaboration/stats', (req, res) => {
    res.json(collaborationManager.getCollaborationStats());
  });

  router.get('/api/collaboration/history', (req, res) => {
    const { agentId, limit } = req.query;
    res.json(collaborationManager.getCollaborationHistory(agentId, parseInt(limit || 50)));
  });

  return router;
}

export default createAPIRouter;
