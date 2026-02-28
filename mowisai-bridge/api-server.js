#!/usr/bin/env node

// API Server
// Provides REST endpoints for all MowisAI features

import express from 'express';
import cors from 'cors';
import { v4 as uuidv4 } from 'uuid';
import createAPIRouter from './api.js';
import { streamingManager } from './streaming.js';
import { config } from './config.js';
import { monitor } from './monitor.js';

const app = express();
const PORT = process.env.PORT || 3000;

// Middleware
app.use(cors());
app.use(express.json());

// Logging middleware
app.use((req, res, next) => {
  console.log(`[${new Date().toISOString()}] ${req.method} ${req.path}`);
  next();
});

// Health check
app.get('/health', (req, res) => {
  res.json({
    status: 'ok',
    uptime: process.uptime(),
    timestamp: new Date().toISOString(),
    version: '1.0.0'
  });
});

// API Routes
const apiRouter = createAPIRouter();
app.use(apiRouter);

// SSE Streaming endpoint
app.get('/stream/:connectionId', (req, res) => {
  const { connectionId } = req.params;
  const createSSEStream = streamingManager.createSSEStream(connectionId);
  createSSEStream(req, res);
});

// Dashboard endpoint
app.get('/api/dashboard', (req, res) => {
  res.json({
    config: config.toJSON(),
    monitoring: monitor.getDashboardData(),
    streaming: streamingManager.getStreamingStats(),
    timestamp: new Date().toISOString()
  });
});

// Error handling
app.use((err, req, res, next) => {
  console.error(`Error: ${err.message}`);
  res.status(500).json({
    error: err.message,
    timestamp: new Date().toISOString()
  });
});

// 404 handler
app.use((req, res) => {
  res.status(404).json({
    error: 'Not found',
    path: req.path,
    method: req.method
  });
});

// Start server
const server = app.listen(PORT, () => {
  console.log(`
╔═══════════════════════════════════════════════════════════╗
║         MowisAI Engine - API Server Started              ║
╠═══════════════════════════════════════════════════════════╣
║ Server:        http://localhost:${PORT}                   
║ Health:        http://localhost:${PORT}/health            
║ Dashboard:     http://localhost:${PORT}/api/dashboard     
║ Documentation: http://localhost:${PORT}/api/docs (coming soon)
╚═══════════════════════════════════════════════════════════╝
  `);
});

// Graceful shutdown
process.on('SIGTERM', () => {
  console.log('SIGTERM received, shutting down gracefully...');
  server.close(() => {
    console.log('Server closed');
    process.exit(0);
  });
});

export default app;
