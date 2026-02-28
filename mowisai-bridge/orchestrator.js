#!/usr/bin/env node

// -----------------------------------------------------------------------------
// Orchestrator for the MowisAI bridge
// -----------------------------------------------------------------------------
// This script coordinates the multi-agent workflow inside the bridge container.
// It communicates with the MowisAI engine over a UNIX socket, spawns sandboxed
// agents (planner, coder, researcher, reviewer, hub, etc.), and manages the
// complete lifecycle of a user task:
//   1. Planner analysis and task classification
//   2. Dynamic agent spawning based on planner recommendations
//   3. Execution loops for coder/researcher agents with reviewer feedback
//   4. File creation, build/test/run, and exporting of results to disk
//   5. Final summarization and cleanup
//
// The orchestrator also includes utility helpers for session commands, inbox
// messaging, sandbox management, and professional file conversions.
//
// Recent improvements:
//   * Fixed a missing closing brace in the file‑generation loop that caused
//     build/test commands to run once per file instead of once per task.
//   * Simplified JSON export logic to avoid unnecessary CSV conversion.
//   * Added detailed comments and refactored for readability.
//
// NOTE: This file is executed as a CLI tool (see runOrchestrator at the bottom).

import Groq from 'groq-sdk';
import net from 'net';
import { v4 as uuidv4 } from 'uuid';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import FileConverterClient from '../mcp-file-converter/client.js';
import {
  generateBarChart,
  generatePieChart,
  generateLineChart,
  generateComparisonChart,
  generateDoughnutChart
} from '../mcp-file-converter/chart-generator.js';
import {
  searchWeb,
  fetchStockData,
  fetchWeather,
  fetchCryptoPrices,
  fetchGitHubTrending,
  fetchNews
} from '../mcp-file-converter/data-fetcher.js';
import {
  getImageForDocument,
  processImages,
  createCollage
} from '../mcp-file-converter/image-handler.js';

// Socket path for the MowisAI Engine
const SOCKET_PATH = '/tmp/mowisai.sock';

// Groq model configuration
const MODEL = 'llama-3.3-70b-versatile';

// Default resource limits
const DEFAULT_MEMORY_MB = 512;
const DEFAULT_CPU_PERCENT = 50;

// Maximum review iterations
const MAX_REVIEW_ITERATIONS = 3;

// Agent type definitions with specialized prompts for different domains
const AGENT_TYPES = {
  coder: {
    name: 'Coder',
    systemPrompt: `You are a CODER agent specialized in software development.
Your role is to write code, create files, and implement technical solutions.
You have access to shell commands and file operations.
Focus on: correctness, best practices, testing, clean code.`,
    canCreateFiles: true,
    reviewCriteria: 'Check for syntax errors, logic bugs, missing tests, and code quality issues. Only reject for actual problems, not style preferences.',
    fileExtensions: ['.js', '.ts', '.py', '.go', '.rs', '.java', '.c', '.cpp', '.rb', '.php']
  },
  financial_analyst: {
    name: 'Financial Analyst',
    systemPrompt: `You are a FINANCIAL ANALYST agent specialized in financial modeling and analysis.
Your role is to analyze financial data, create spreadsheets, build financial models, and generate reports.
You can use calculators, create CSV/Excel files, and perform calculations.

🎨 PROFESSIONAL OUTPUT CAPABILITIES:
- Include CHARTS in Excel/PowerPoint files (specify chart type in output comments)
- Create structured JSON/CSV data for visualization
- Generate reports as .xlsx, .pdf, .docx, or .pptx files
- Access web data: stock prices, market data, economic indicators

When creating Excel files, structure data as JSON arrays:
[{"Year": 2024, "Revenue": 1000000}, {"Year": 2025, "Revenue": 1200000}]
Comments can include: "CHART_TYPE: bar|pie|line" to request visualizations.

Focus on: accuracy, sound methodology, clear assumptions, professional presentation.`,
    canCreateFiles: true,
    reviewCriteria: 'Verify calculations are correct, check assumptions are reasonable, validate methodology is sound. Only reject for factual errors or flawed logic.',
    fileExtensions: ['.csv', '.xlsx', '.json', '.md', '.pdf', '.docx', '.pptx']
  },
  designer: {
    name: 'Designer',
    systemPrompt: `You are a DESIGNER agent specialized in creating visual assets and documents.
Your role is to generate images, layouts, presentations, PDFs, and design files.
You have access to imagemagick for images, pandoc for documents, and file operations.

🎨 PROFESSIONAL VISUAL CAPABILITIES:
- Create PowerPoint presentations (.pptx) with structured slide content
- Design PDF reports with professional layouts
- Generate visual documents with charts and images
- Output formats: .pptx, .pdf, .docx with embedded visuals

For .pptx files, structure as a slide array:
[{"title": "Title Slide", "content": "..."}, {"title": "Data Slide", "content": "..."}]

For professional visual impact, request charts to be embedded automatically.

Focus on: visual quality, format compliance, professional appearance.`,
    canCreateFiles: true,
    reviewCriteria: 'Check output files exist and are valid, verify formats are correct, assess visual quality is professional. Only reject for actual output problems.',
    fileExtensions: ['.svg', '.png', '.jpg', '.pdf', '.html', '.md', '.pptx', '.docx']
  },
  researcher: {
    name: 'Researcher',
    systemPrompt: `You are a RESEARCHER agent specialized in information gathering and analysis.
Your role is to find, analyze, and synthesize information from various sources.
You perform literature reviews, competitive analysis, and fact-finding missions.

🌐 WEB INTEGRATION CAPABILITIES:
- Search the web for information (uses DuckDuckGo, no API key needed)
- Fetch trending GitHub repositories
- Get latest technology news
- Access real-time data and market information
- Download images from URLs for reports

When researching, structure findings as JSON for easy conversion:
{"research_topic": "...", "sources": [...], "findings": [...], "visualizations": [...]}

Generate reports as .md, .docx, or .pdf files with structured data.

Focus on: source credibility, factual accuracy, comprehensive coverage, objective analysis.`,
    canCreateFiles: true,
    reviewCriteria: 'Verify claims are supported, check reasoning is sound, identify any gaps in coverage. Only reject for unsupported claims or flawed logic.',
    fileExtensions: ['.md', '.txt', '.json', '.csv', '.pdf', '.docx', '.pptx']
  },
  writer: {
    name: 'Writer',
    systemPrompt: `You are a WRITER agent specialized in content creation.
Your role is to write documents, articles, copy, documentation, and creative content.
You craft clear, engaging, and appropriate content for the target audience.

📝 PROFESSIONAL DOCUMENT CAPABILITIES:
- Create Word documents (.docx) with structured sections
- Generate PDF reports with professional formatting
- Build PowerPoint presentations (.pptx) with slides
- Embed images and charts for visual impact

For .docx files, use this structure:
{"title": "Document Title", "sections": [{"heading": "Section 1", "content": "..."}]}

For .pptx files, use slides structure for engaging presentations.

Generate professional reports with proper formatting, styling, and visual elements.

Focus on: clarity, grammar, structure, tone, audience appropriateness.`,
    canCreateFiles: true,
    reviewCriteria: 'Check grammar and spelling, assess clarity and flow, verify structure is logical. Only reject for significant quality issues, not minor stylistic preferences.',
    fileExtensions: ['.md', '.txt', '.docx', '.html', '.pdf', '.pptx']
  },
  data_scientist: {
    name: 'Data Scientist',
    systemPrompt: `You are a DATA SCIENTIST agent specialized in data analysis and machine learning.
Your role is to analyze datasets, create visualizations, build models, and derive insights.
You can use Python with pandas, numpy, matplotlib, and other data tools.

📊 CHART & VISUALIZATION CAPABILITIES:
- Request professional chart generation (bar, pie, line, doughnut charts)
- Include charts in Excel, PowerPoint, and PDF reports
- Create data visualizations for presentations
- Structure data as JSON for automatic charting

Format data for visualization:
{"chart_type": "bar", "data": [{"label": "Q1", "value": 50000}], "title": "Quarterly Sales"}

The system can automatically generate charts and embed them in documents.

Generate insights with professional visualizations and reports (.xlsx, .pptx, .pdf, .docx).

Focus on: analytical rigor, reproducibility, clear insights, proper methodology.`,
    canCreateFiles: true,
    reviewCriteria: 'Check analysis methodology is sound, verify visualizations are clear, validate conclusions are supported by data. Only reject for methodological errors.',
    fileExtensions: ['.py', '.ipynb', '.csv', '.json', '.png', '.xlsx', '.pptx', '.pdf', '.docx']
  }
};

// Agent type tracking
let agentTypes = {}; // Maps agentName -> agentType

// Default agents (legacy support)
const AGENTS = ['planner', 'coder', 'reviewer', 'hub'];


// Global state
let sandboxName = null;
let agentSessions = {};

/**
 * Send a request to the MowisAI Engine via Unix socket
 */
async function sendToEngine(request) {
  return new Promise((resolve, reject) => {
    const client = net.createConnection(SOCKET_PATH, () => {
      console.error(`[Orchestrator] → ${request.request_type}`);
      client.write(JSON.stringify(request) + '\n');
    });

    let responseData = '';

    client.on('data', (data) => {
      responseData += data.toString();
      const newlineIndex = responseData.indexOf('\n');
      if (newlineIndex !== -1) {
        client.end();
        const jsonLine = responseData.substring(0, newlineIndex).trim();
        try {
          const response = JSON.parse(jsonLine);
            // Only log concise status; if it's an error, include the message
            if (response && String(response.status).toLowerCase() === 'error') {
              console.error(`[Orchestrator] ← ERROR: ${response.output || response.status}`);
            } else {
              console.error(`[Orchestrator] ← ${response.status}`);
            }
          resolve(response);
        } catch (err) {
          reject(new Error(`Failed to parse response: ${err.message}`));
        }
      }
    });

    client.on('error', (err) => {
      reject(new Error(`Socket error: ${err.message}. Is the engine running?`));
    });

    client.on('close', () => {
      if (responseData.indexOf('\n') === -1 && responseData.length > 0) {
        reject(new Error('Socket closed without complete response'));
      }
    });

    setTimeout(() => {
      client.destroy();
      reject(new Error('Connection timeout'));
    }, 60000);
  });
}

/**
 * Run a command inside an agent's container
 */
async function runInSession(agentName, command, timeoutSecs = 30) {
  const sessionId = agentSessions[agentName];
  if (!sessionId) {
    throw new Error(`No session found for ${agentName}`);
  }

  const response = await sendToEngine({
    request_type: 'run_in_session',
    task_id: uuidv4(),
    session_id: sessionId,
    command: command,
    timeout_secs: timeoutSecs
  });

  if (response.status === 'error') {
    // Handle busybox limitation: "Error: No such file or directory (os error 2)" is engine limitation, not real error
    if (response.output && response.output.trim() === 'Error: No such file or directory (os error 2)') {
      return '';
    }
    throw new Error(`Command failed in ${agentName}: ${response.output}`);
  }

  return response.output;
}

// Wrapper that runs a command in a session and immediately logs the command
// along with its stdout/stderr in a terminal‑like format. This ensures every
// runInSession call has its output visible without having to remember to call
// logSessionOutput manually.
async function runInSessionWithLog(agentName, command, timeoutSecs = 30) {
  const output = await runInSession(agentName, command, timeoutSecs);
  logSessionOutput(command, output);
  return output;
}

// Helper used throughout executeCoder/reviewer to echo the command and its real stdout
function logSessionOutput(command, output) {
  console.error(`$ ${command}`);
  if (output === undefined || output === null || output === '') {
    return;
  }
  const text = String(output);
  const lines = text.split('\n');
  for (const line of lines) {
    if (line.includes('No such file or directory (os error 2)')) {
      // warn if the line contains the OS error message anywhere
      console.error(`⚠️ ${line}`);
    } else {
      console.error(`> ${line}`);
    }
  }
}


/**
 * Create a sandbox for all agents
 */
async function createSandbox() {
  const sandboxId = `sandbox-${uuidv4()}`;
  console.error(`[Orchestrator] Creating sandbox: ${sandboxId}`);
  
  const response = await sendToEngine({
    request_type: 'create_sandbox',
    task_id: uuidv4(),
    sandbox_name: sandboxId
  });
  
  if (response.status === 'error') {
    throw new Error(`Failed to create sandbox: ${response.output}`);
  }
  
  console.error(`[Orchestrator] ✅ Sandbox created: ${sandboxId}`);
  return sandboxId;
}

/**
 * Spawn an agent into the sandbox
 */
async function spawnAgent(agentName) {
  console.error(`[Orchestrator] Spawning agent: ${agentName}`);
  
  const response = await sendToEngine({
    request_type: 'join_sandbox',
    task_id: uuidv4(),
    sandbox_name: sandboxName,
    agent_name: agentName,
    memory_mb: DEFAULT_MEMORY_MB,
    cpu_percent: DEFAULT_CPU_PERCENT
  });
  
  if (response.status === 'error') {
    throw new Error(`Failed to spawn ${agentName}: ${response.output}`);
  }
  
  agentSessions[agentName] = response.session_id;
  console.error(`[Orchestrator] ✅ ${agentName} ready (session: ${response.session_id})`);
  return response.session_id;
}

/**
 * Send a message to an agent
 */
async function sendMessage(fromAgent, toAgent, content) {
  const response = await sendToEngine({
    request_type: 'message_send',
    task_id: uuidv4(),
    sandbox_name: sandboxName,
    from_agent: fromAgent,
    to_agent: toAgent,
    content: content
  });

  // If messaging fails, return false instead of throwing so callers can handle gracefully
  if (!response || String(response.status).toLowerCase() === 'error') {
    console.error(`[Orchestrator] Warning: message_send to ${toAgent} failed: ${response?.output || 'no response'}`);
    return false;
  }

  return true;
}

/**
 * Read an agent's inbox
 */
async function readInbox(agentName) {
  const response = await sendToEngine({
    request_type: 'message_read',
    task_id: uuidv4(),
    sandbox_name: sandboxName,
    agent_name: agentName
  });
  
  if (response.status === 'error') {
    console.error(`[Orchestrator] Warning: Failed to read ${agentName} inbox: ${response.output}`);
    return [];
  }
  
  try {
    const messages = JSON.parse(response.output);
    return messages;
  } catch (err) {
    console.error(`[Orchestrator] Warning: Failed to parse messages: ${err.message}`);
    return [];
  }
}

/**
 * Kill an agent's session
 */
async function killAgent(agentName) {
  const sessionId = agentSessions[agentName];
  if (!sessionId) return;
  
  console.error(`[Orchestrator] Killing ${agentName}...`);
  await sendToEngine({
    request_type: 'kill_session',
    task_id: uuidv4(),
    session_id: sessionId
  });
  delete agentSessions[agentName];
}

/**
 * Execute planner: creates structured plan with task classification and agent recommendations
 */
async function executePlanner(groq, userTask) {
  console.error(`\n📝 Step 1: Running PLANNER...`);

  const systemPrompt = `You are the PLANNER agent. Analyze the task and output a structured implementation plan.

## Task Classification
First, classify the task type:
- CODING: Software development, programming, scripts, APIs, websites, apps
- FINANCIAL_ANALYSIS: Budgets, forecasts, valuations, market analysis, financial modeling
- DESIGN: Visual assets, presentations, layouts, graphics, logos, documents
- RESEARCH: Information gathering, fact-finding, competitive analysis, literature review
- WRITING: Content creation, documentation, articles, copywriting
- DATA_SCIENCE: Data analysis, visualization, machine learning, statistics
- MIXED: Combination of multiple types

## Agent Selection
Based on the task, recommend which agent types are needed. Available types:
- coder: Software development and technical implementation
- financial_analyst: Financial modeling and business analysis
- designer: Visual design and document creation
- researcher: Information gathering and analysis
- writer: Content creation and writing
- data_scientist: Data analysis and ML

## Output Format
TASK_TYPE: (the classification)
RECOMMENDED_AGENTS: (JSON array of agents needed)
  Example: [{"agent_type": "coder", "role": "build API"}, {"agent_type": "writer", "role": "write documentation"}]
  For simple tasks: [{"agent_type": "coder", "role": "implement solution"}]
OBJECTIVE: (clear statement of what needs to be accomplished)
DELIVERABLES: (specific outputs expected)
APPROACH: (high-level strategy)
SETUP_COMMANDS: (commands to prepare environment, if any)
FILES: (files to create, if applicable)
BUILD_COMMAND: (if applicable, else "none")
TEST_COMMAND: (if applicable, else "none")
RUN_COMMAND: (to view/execute result, if applicable, else "none")
IMPLEMENTATION: (step-by-step plan)

Be specific about which agent types should handle which parts of the task.`;
  
  const response = await groq.chat.completions.create({
    model: MODEL,
    messages: [
      { role: 'system', content: systemPrompt },
      { role: 'user', content: `Analyze and plan: ${userTask}` }
    ],
    temperature: 0.7
  });

  const plan = response.choices[0].message.content;
  console.error(`[Orchestrator] Planner created plan (${plan.length} chars)`);

  // Parse RECOMMENDED_AGENTS from plan
  let recommendedAgents = [{ agent_type: 'coder', role: 'implement solution' }]; // default fallback

  const agentsMatch = plan.match(/RECOMMENDED_AGENTS:\s*(```json\s*)?([\s\S]*?)(```)?\s*(?=\n[A-Z]|$)/i);
  if (agentsMatch) {
    try {
      const jsonStr = agentsMatch[2].trim();
      // Try to extract JSON array
      const jsonStart = jsonStr.indexOf('[');
      const jsonEnd = jsonStr.lastIndexOf(']');
      if (jsonStart !== -1 && jsonEnd !== -1 && jsonEnd > jsonStart) {
        const parsed = JSON.parse(jsonStr.substring(jsonStart, jsonEnd + 1));
        if (Array.isArray(parsed) && parsed.length > 0) {
          recommendedAgents = parsed;
          console.error(`[Orchestrator] Planner recommends ${recommendedAgents.length} agent(s): ${recommendedAgents.map(a => a.agent_type).join(', ')}`);
        }
      }
    } catch (e) {
      console.error('[Orchestrator] Warning: Could not parse RECOMMENDED_AGENTS, using default coder');
    }
  }

  // Parse TASK_TYPE
  const taskTypeMatch = plan.match(/TASK_TYPE:\s*(.+)/i);
  const taskType = taskTypeMatch ? taskTypeMatch[1].trim() : 'CODING';
  console.error(`[Orchestrator] Task classified as: ${taskType}`);

  // Planner returns the plan directly to the caller; do not attempt to message the hub
  // here because the planner is not an active sandbox participant at this stage.
  console.error(`[Orchestrator] Planner prepared plan (not sending to hub directly)`);

  return { plan, taskType, recommendedAgents };
}

/**
 * Extract project directory from setup commands
 * Detects if a scaffolding tool (vite, create, init, new) created a project subdirectory
 * Returns the directory name if found, otherwise null
 */
function extractProjectDirectory(setupCommands) {
  if (!Array.isArray(setupCommands) || setupCommands.length === 0) {
    return null;
  }

  // Look for patterns like: vite, create, init, new followed by a project name
  for (const cmd of setupCommands) {
    // Pattern: npm create vite@latest my-app -- --template react
    // Handles @latest and stops at --
    let match = cmd.match(/npm\s+create\s+\S+\s+(\S+?)(?:\s+--|$)/i);
    if (match) return match[1];

    // Pattern: npx create-react-app my-app
    match = cmd.match(/create-react-app\s+(\S+)/i);
    if (match) return match[1];

    // Pattern: pip virtualenv venv OR python -m venv venv
    match = cmd.match(/(?:virtualenv|venv|(?:-m\s+)?venv)\s+(\S+)/i);
    if (match) return match[1];

    // Pattern: cargo init my-project
    match = cmd.match(/cargo\s+init\s+(\S+)/i);
    if (match) return match[1];

    // Pattern: rails new my-app
    match = cmd.match(/rails\s+new\s+(\S+)/i);
    if (match) return match[1];

    // Pattern: django-admin startproject myproject
    match = cmd.match(/(?:django-admin|manage\.py)\s+startproject\s+(\S+)/i);
    if (match) return match[1];
  }

  return null;
}

/**
 * Execute coder: receives plan and optional feedback, executes setup commands, creates files, runs solution
 */
async function executeCoder(groq, userTask, plan, agentName = 'coder', previousAttempt = null, feedback = null) {

  console.error(`\n💻 Step 2: Running CODER...`);
  
  // Cleanup: Remove previous files/builds but preserve dependencies for reuse
  console.error(`[Orchestrator] Cleaning up old files...`);
  try {
    // Full cleanup for fresh start - remove previous outputs
    const cleanupCmd = 'rm -rf *.py *.js *.ts *.jsx *.tsx *.sh *.go *.rs *.c *.cpp *.h *.hpp *.java *.rb *.php *.pl *.cs *.swift *.kt *.scala *.r *.m dist build src target bin obj *.xlsx *.docx *.pptx *.pdf *.csv *.json 2>/dev/null; echo "cleanup done"';
    const cleanupOutput = await runInSessionWithLog(agentName, cleanupCmd, 5);
    console.error(`[Orchestrator] ✅ Old files cleaned up`);
  } catch (err) {
    logSessionOutput('cleanup', err.message);
    console.error(`[Orchestrator] ⚠️ Cleanup warning: ${err.message}`);
  }
  
  // Step 1: Generate the solution structure (filenames and setup commands only)
  const structureSystemPrompt = `You are the CODER agent. Given a task and implementation plan, output ONLY a JSON object with these fields:
- "setup_commands": array of shell commands to set up the environment
- "files": array of filenames to create (include paths like "src/utils.js" for multi-file projects)
- "build_command": build/compile command (use "none" if not needed)
- "test_command": test command (use "none" if not applicable)
- "run_command": command to run the application

IMPORTANT - Working Directory Handling:
When using Vite or any scaffolding tool, your setup_commands should only scaffold the project. Do NOT include cd as a setup command. The orchestrator will automatically handle working directories. All filenames in the files array should be relative paths like src/App.tsx, not absolute paths.

Output ONLY valid JSON. No markdown, no explanation. Example for a Python project:
{"setup_commands":["pip install flask"],"files":["app.py","utils.py"],"build_command":"none","test_command":"pytest","run_command":"python app.py"}`;

  
  let structurePrompt = `Task: ${userTask}

Implementation plan from planner:
${plan}`;

  if (previousAttempt && feedback) {
    structurePrompt += `

⚠️ PREVIOUS ATTEMPT FAILED - REVIEWER FEEDBACK:
${feedback}

Your previous code had issues. Fix them and regenerate the complete solution structure.`;
  } else {
    structurePrompt += `

Generate the JSON with setup_commands, files (just filenames), and run_command.`;
  }
  
  const structureResponse = await groq.chat.completions.create({
    model: MODEL,
    messages: [
      { role: 'system', content: structureSystemPrompt },
      { role: 'user', content: structurePrompt }
    ],
    temperature: 0.7
  });
  
  // Parse the structure JSON
  let coderStructure;
  try {
    const rawResponse = structureResponse.choices[0].message.content.trim();
    const cleaned = rawResponse.replace(/\\n/g, ' ').replace(/[\x00-\x1F]/g, ' ');
    const jsonMatch = cleaned.match(/```json\s*([\s\S]*?)\s*```/) || cleaned.match(/```\s*([\s\S]*?)\s*```/) || [null, cleaned];
    coderStructure = JSON.parse(jsonMatch[1] || cleaned);

    if (!coderStructure.setup_commands || !coderStructure.files || !coderStructure.run_command) {
      throw new Error('Response must include setup_commands, files, and run_command');
    }

    // Set optional fields if not present
    if (!coderStructure.build_command) coderStructure.build_command = null;
    if (!coderStructure.test_command) coderStructure.test_command = null;
  } catch (err) {
    console.error(`[Orchestrator] ❌ Failed to parse coder structure: ${err.message}`);
    console.error(`[Orchestrator] Raw response: ${structureResponse.choices[0].message.content.substring(0, 200)}...`);
    throw new Error('Coder failed to generate valid structure');
  }
  
  console.error(`[Orchestrator] Coder structure: ${coderStructure.setup_commands.length} setup commands, ${coderStructure.files.length} files`);
  
  // Execute setup commands and detect project directory
  const stepOutputs = [];
  let workDir = null;
  for (let i = 0; i < coderStructure.setup_commands.length; i++) {
    let cmd = coderStructure.setup_commands[i];
    
    // On first command, try to extract project directory from it
    if (i === 0) {
      workDir = extractProjectDirectory([cmd]);
      if (workDir) {
        console.error(`[Orchestrator] 📁 Detected project directory: ${workDir}`);
      }
    }
    
    // For commands after the first, prepend cd if workDir was detected
    if (i > 0 && workDir) {
      cmd = `cd /root/${workDir} && ${cmd}`;
    }
    
    console.error(`[Orchestrator] Setup ${i + 1}/${coderStructure.setup_commands.length}: ${cmd}`);
    
    try {
      // Extended timeout for setup commands (npm install, etc.)
      const isLongRunning = cmd.includes('npm') || cmd.includes('pip') || cmd.includes('install') || cmd.includes('create');
      const timeout = isLongRunning ? 300 : 60;
      const output = await runInSessionWithLog(agentName, cmd, timeout);
      stepOutputs.push({
        step: i + 1,
        description: `Setup: ${coderStructure.setup_commands[i]}`,
        command: coderStructure.setup_commands[i],
        output: output,
        status: 'success'
      });
      // success message retained but output has already been shown above
      console.error(`[Orchestrator] ✅ Setup ${i + 1} completed`);
    } catch (err) {
      logSessionOutput(cmd, err.message);
      stepOutputs.push({
        step: i + 1,
        description: `Setup: ${coderStructure.setup_commands[i]}`,
        command: coderStructure.setup_commands[i],
        output: err.message,
        status: 'error'
      });
      console.error(`[Orchestrator] ❌ Setup ${i + 1} failed: ${err.message}`);
    }
  }

  // Step 2: Generate each file's content individually
  const files = {};
  const allFilenames = coderStructure.files;

  for (let i = 0; i < allFilenames.length; i++) {
    const filename = allFilenames[i];
    console.error(`[Orchestrator] Generating content for: ${filename} (${i + 1}/${allFilenames.length})`);

    // Determine file format and use appropriate prompt
    const fileExt = filename.toLowerCase().split('.').pop();
    let fileSystemPrompt;
    let formatInstructions = '';

    if (fileExt === 'xlsx') {
      fileSystemPrompt = `You are a FINANCIAL ANALYST agent creating an Excel spreadsheet.
IMPORTANT: Output ONLY valid JSON representing tabular data. NO explanations, NO markdown, NO text.
Do NOT include any explanatory text or descriptions.

Return JSON as an array of objects with consistent keys representing rows and columns.
Example: [{"Year":2024,"Revenue":1000000,"Profit":300000},{"Year":2025,"Revenue":1500000,"Profit":450000}]`;
      formatInstructions = `\n\nFOR EXCEL FILES (.xlsx):
Return ONLY JSON data. Structure as an array of objects where each object is a row.
Example format:
[
  {"Column1": "value", "Column2": "value"},
  {"Column1": "value", "Column2": "value"}
]`;
    } else if (fileExt === 'pdf') {
      fileSystemPrompt = `You are a WRITER agent creating a PDF document.
Output ONLY the content text. NO JSON, NO markdown, NO code.
Write professional report content with clear structure.`;
      formatInstructions = `\n\nFOR PDF FILES (.pdf):
Just write the report content naturally. Format will be converted to PDF.
Use blank lines to separate paragraphs.`;
    } else if (fileExt === 'docx') {
      fileSystemPrompt = `You are a WRITER agent creating a Word document.
Output ONLY valid JSON with document structure. NO explanations, NO markdown.
Structure as an object with title, sections, and content.`;
      formatInstructions = `\n\nFOR WORD DOCUMENTS (.docx):
Return JSON with this structure:
{
  "title": "Document Title",
  "sections": [
    {"heading": "Section 1", "content": "..."},
    {"heading": "Section 2", "content": "..."}
  ]
}`;
    } else if (fileExt === 'pptx') {
      fileSystemPrompt = `You are a DATA SCIENTIST agent creating a PowerPoint presentation.
Output ONLY valid JSON representing slides. NO explanations, NO markdown, NO code.`;
      formatInstructions = `\n\nFOR POWERPOINT FILES (.pptx):
Return JSON as an array of slide objects:
[
  {"title": "Slide 1 Title", "content": "Slide 1 content..."},
  {"title": "Slide 2 Title", "content": "Slide 2 content..."}
]`;
    } else if (fileExt === 'csv') {
      fileSystemPrompt = `You are a DATA ANALYST agent creating a CSV file.
Output ONLY CSV data. NO JSON, NO explanations, NO markdown.
First row should be headers.`;
      formatInstructions = `\n\nFOR CSV FILES (.csv):
Return raw CSV with headers in first row.`;
    } else {
      fileSystemPrompt = `You are a CODER agent. Generate the complete content for a file. Output ONLY the raw file content.`;
    }

    // Build context about other files for large projects
    let otherFilesContext = '';
    if (allFilenames.length > 1) {
      const otherFiles = allFilenames.filter(f => f !== filename);
      otherFilesContext = `\n\nOther files in project:\n${otherFiles.join('\n')}`;
    }

    // Include previously generated files for context
    let generatedContext = '';
    if (Object.keys(files).length > 0) {
      generatedContext = `\n\nPreviously generated files:\n${Object.keys(files).join(', ')}`;
    }

    const filePrompt = `Task: ${userTask}

Implementation plan:
${plan}${otherFilesContext}${generatedContext}${formatInstructions}

Generate the complete content for file: ${filename}

CRITICAL: Return ONLY the content in the specified format. NO explanations, NO preamble, NO extra text.`;
    
    const fileResponse = await groq.chat.completions.create({
      model: MODEL,
      messages: [
        { role: 'system', content: fileSystemPrompt },
        { role: 'user', content: filePrompt }
      ],
      temperature: 0.7
    });
    
    let fileContent = fileResponse.choices[0].message.content.trim();
    
    // CRITICAL FIX: Strip markdown code fence markers if present
    // Remove ```language\n at start and ``` at end
    fileContent = fileContent.replace(/^```[\w]*\n?/, '').replace(/\n?```$/, '');
    
    console.error(`[Orchestrator] Generated ${filename} (${fileContent.length} chars)`);
    
    // Write the file using heredoc (safer for large content)
      // Escape special characters for heredoc
      const escapedContent = fileContent
        .replace(/\\/g, '\\\\')
        .replace(/\$/g, '\\$')
        .replace(/`/g, '\\`');
      
      // Prepend cd command if project directory was detected
      let writeCommand = `cat > ${filename} << 'EOF__MOWISAI'\n${escapedContent}\nEOF__MOWISAI`;
      if (workDir) {
        writeCommand = `cd /root/${workDir} && ${writeCommand}`;
      }
      try {
        const writeOutput = await runInSessionWithLog(agentName, writeCommand, 10);
        files[filename] = fileContent;
        console.error(`[Orchestrator] ✅ File written: ${filename}`);
      } catch (err) {
        logSessionOutput(writeCommand, err.message);
        console.error(`[Orchestrator] ❌ Failed to write ${filename}: ${err.message}`);
      }
    }

  // Build verification (if build command exists and is not "none")
  if (coderStructure.build_command && coderStructure.build_command !== 'none') {
    console.error(`[Orchestrator] Building project: ${coderStructure.build_command}`);
    try {
      // Prepend cd command if project directory was detected
      let buildCmd = coderStructure.build_command;
      if (workDir) {
        buildCmd = `cd /root/${workDir} && ${buildCmd}`;
      }
      const buildOutput = await runInSessionWithLog(agentName, buildCmd, 120);
      stepOutputs.push({
        step: stepOutputs.length + 1,
        description: 'Build project',
        command: coderStructure.build_command,
        output: buildOutput,
        status: 'success'
      });
      // buildOutput already logged above in terminal style
    } catch (err) {
      logSessionOutput(buildCmd, err.message);
      stepOutputs.push({
        step: stepOutputs.length + 1,
        description: 'Build project',
        command: coderStructure.build_command,
        output: err.message,
        status: 'error'
      });
      console.error(`[Orchestrator] ❌ Build failed: ${err.message}`);
    }
  }

  // Test execution (if test command exists and is not "none")
  if (coderStructure.test_command && coderStructure.test_command !== 'none') {
    console.error(`[Orchestrator] Running tests: ${coderStructure.test_command}`);
    try {
      // Prepend cd command if project directory was detected
      let testCmd = coderStructure.test_command;
      if (workDir) {
        testCmd = `cd /root/${workDir} && ${testCmd}`;
      }
      const testOutput = await runInSessionWithLog(agentName, testCmd, 120);
      stepOutputs.push({
        step: stepOutputs.length + 1,
        description: 'Run tests',
        command: coderStructure.test_command,
        output: testOutput,
        status: 'success'
      });
      // output displayed above
    } catch (err) {
      logSessionOutput(testCmd, err.message);
      stepOutputs.push({
        step: stepOutputs.length + 1,
        description: 'Run tests',
        command: coderStructure.test_command,
        output: err.message,
        status: 'error'
      });
      console.error(`[Orchestrator] ⚠️ Tests failed or not found`);
      console.error(`[Orchestrator] Error: ${err.message}\n`);
    }
  }

  // Run the solution
  console.error(`[Orchestrator] Running solution: ${coderStructure.run_command}`);
  let finalOutput = '';
  try {
    // Prepend cd command if project directory was detected
    let runCmd = coderStructure.run_command;
    if (workDir) {
      runCmd = `cd /root/${workDir} && ${runCmd}`;
    }
    finalOutput = await runInSessionWithLog(agentName, runCmd, 60);
    stepOutputs.push({
      step: stepOutputs.length + 1,
      description: 'Run solution',
      command: coderStructure.run_command,
      output: finalOutput,
      status: 'success'
    });
    // output displayed above
  } catch (err) {
    logSessionOutput(runCmd, err.message);
    stepOutputs.push({
      step: stepOutputs.length + 1,
      description: 'Run solution',
      command: coderStructure.run_command,
      output: err.message,
      status: 'error'
    });
    console.error(`[Orchestrator] ❌ Solution failed: ${err.message}`);
  }


  
  // Files are already collected during the write phase above
  console.error(`[Orchestrator] Collected ${Object.keys(files).length} files`);


  
  // Send results to hub
  const message = JSON.stringify({
    files: files,
    steps: stepOutputs,
    finalOutput: stepOutputs.length > 0 ? stepOutputs[stepOutputs.length - 1].output : 'No output'
  }, null, 2);
  
  await sendMessage(agentName, 'hub', message);
  console.error(`[Orchestrator] ✅ Coder sent results to hub`);
  
  return { files, steps: stepOutputs };
}

/**
 * Execute researcher: performs actual web search, data fetching, information gathering
 * Uses data-fetcher module for real web integration
 */
async function executeResearcher(groq, userTask, plan, agentName = 'researcher-1', previousAttempt = null, feedback = null) {
  console.error(`\n🔍 Step 2: Running RESEARCHER...`);
  
  // Parse task to determine what to research
  const researchPrompt = `Based on this task, extract the key research topics and data sources needed:

Task: ${userTask}

Plan:
${plan}

${feedback ? `Previous feedback: ${feedback}` : ''}

Output JSON with these fields (ONLY JSON, no markdown):
{
  "research_topics": ["topic1", "topic2"],
  "data_sources": ["web_search", "stock_data", "news", "github", "weather", "crypto"],
  "search_queries": ["query1", "query2"],
  "output_file": "research_report.pdf"
}`;

  const researchResponse = await groq.chat.completions.create({
    model: MODEL,
    messages: [
      { role: 'system', content: 'You are a research coordinator. Output ONLY valid JSON.' },
      { role: 'user', content: researchPrompt }
    ],
    temperature: 0.7
  });

  let researchPlan;
  try {
    const rawResponse = researchResponse.choices[0].message.content.trim();
    const cleaned = rawResponse.replace(/\\n/g, ' ');
    const jsonMatch = cleaned.match(/```json\s*([\s\S]*?)\s*```/) || cleaned.match(/```\s*([\s\S]*?)\s*```/) || [null, cleaned];
    researchPlan = JSON.parse(jsonMatch[1] || cleaned);
  } catch (err) {
    researchPlan = {
      research_topics: ['general research'],
      data_sources: ['web_search'],
      search_queries: [userTask],
      output_file: 'research_report.pdf'
    };
  }

  console.error(`[Orchestrator] Researcher will search: ${researchPlan.search_queries.join(', ')}`);

  // Perform actual data collection using data-fetcher
  const researchData = {
    task: userTask,
    collected_at: new Date().toISOString(),
    search_results: [],
    data_sources: {}
  };

  // Run web searches
  for (const query of researchPlan.search_queries) {
    console.error(`[Orchestrator] 🔍 Web search: "${query}"...`);
    try {
      const results = await searchWeb(query);
      const isFallback = String(results[0]?.source || '').toLowerCase().includes('fallback');
      
      if (isFallback) {
        console.error(`[Orchestrator] ℹ️ Using contextual fallback data for "${query}" (no live internet access)`);
      } else {
        console.error(`[Orchestrator] 🌐 Fetched live results for "${query}"`);
      }
      
      researchData.search_results.push({
        query,
        results: results.slice(0, 5), // Top 5 results
        count: results.length,
        source: isFallback ? 'fallback' : 'live'
      });
      console.error(`[Orchestrator] ✅ Found ${results.length} results for "${query}"`);
    } catch (err) {
      console.error(`[Orchestrator] ⚠️ Web search error: ${err.message}`);
    }
  }

  // Fetch additional data sources if requested
  if (researchPlan.data_sources.includes('github')) {
    console.error(`[Orchestrator] Fetching GitHub trends...`);
    try {
      const trends = await fetchGitHubTrending('javascript');
      researchData.data_sources.github = trends.slice(0, 5);
      console.error(`[Orchestrator] ✅ Fetched GitHub trends`);
    } catch (err) {
      console.error(`[Orchestrator] ⚠️ GitHub fetch failed: ${err.message}`);
    }
  }

  if (researchPlan.data_sources.includes('news')) {
    console.error(`[Orchestrator] Fetching latest news...`);
    try {
      const news = await fetchNews();
      researchData.data_sources.news = news.slice(0, 5);
      console.error(`[Orchestrator] ✅ Fetched news`);
    } catch (err) {
      console.error(`[Orchestrator] ⚠️ News fetch failed: ${err.message}`);
    }
  }

  // Generate report content using LLM
  console.error(`[Orchestrator] Generating research report...`);
  
  const reportPrompt = `Write a professional research report based on these findings:

Task: ${userTask}

Research Data:
${JSON.stringify(researchData, null, 2)}

Write a well-structured report with:
1. Executive Summary
2. Research Methodology
3. Key Findings
4. Analysis
5. Conclusions
6. References

Format as plain text for PDF conversion.`;

  const reportResponse = await groq.chat.completions.create({
    model: MODEL,
    messages: [
      { role: 'system', content: 'You are a professional researcher. Write detailed, well-sourced reports.' },
      { role: 'user', content: reportPrompt }
    ],
    temperature: 0.7
  });

  let reportContent = reportResponse.choices[0].message.content;
  
  // CRITICAL FIX: Strip markdown code fence markers if present
  reportContent = reportContent.replace(/^```[\w]*\n?/, '').replace(/\n?```$/, '');
  
  // Create output file
  const outputFilename = researchPlan.output_file || 'research_report.pdf';
  const files = {
    [outputFilename]: reportContent,
    'research_data.json': JSON.stringify(researchData, null, 2)
  };

  // Send results to hub
  const message = JSON.stringify({
    files: files,
    research_data: researchData,
    report_preview: reportContent.substring(0, 200) + '...'
  }, null, 2);

  await sendMessage(agentName, 'hub', message);
  console.error(`[Orchestrator] ✅ Researcher sent results to hub`);

  return { files };
}

/**
 * Execute reviewer: receives files from hub, writes to own container, runs and reviews
 * Returns: { review: string, approved: boolean, executionOutput: string }
 */
async function executeReviewer(groq, userTask, coderResults, agentType = 'coder') {

  console.error(`\n🔍 Step 3: Running REVIEWER for ${agentType}...`);

  const { files, steps, finalOutput } = coderResults;

  // Get agent type configuration for review criteria
  const typeConfig = AGENT_TYPES[agentType] || AGENT_TYPES.coder;

  // Extract commands from coder results (find from steps if not directly available)
  let buildCommand = null;
  let testCommand = null;
  let runCommand = null;

  // Try to find commands from steps
  for (const step of steps || []) {
    if (step.description === 'Build project') {
      buildCommand = step.command;
    } else if (step.description === 'Run tests') {
      testCommand = step.command;
    } else if (step.description === 'Run solution') {
      runCommand = step.command;
    }
  }

  // Write files to reviewer's container
  console.error(`[Orchestrator] Writing files to reviewer container...`);
  for (const [filename, content] of Object.entries(files)) {
    // Escape special characters for heredoc
    const escapedContent = content
      .replace(/\\/g, '\\\\')
      .replace(/\$/g, '\\$')
      .replace(/`/g, '\\`');

    // Use heredoc to write the file
    const writeCommand = `cat > ${filename} << 'EOF__MOWISAI'\n${escapedContent}\nEOF__MOWISAI`;
    try {
      const writeOut = await runInSessionWithLog('reviewer', writeCommand, 10);
      console.error(`[Orchestrator] ✅ Wrote ${filename} to reviewer container`);
    } catch (err) {
      logSessionOutput(writeCommand, err.message);
      console.error(`[Orchestrator] ❌ Failed to write ${filename} to reviewer container: ${err.message}`);
    }
  }

  let reviewerExecutionOutput = '';

  // Build verification (if there was a build step)
  if (buildCommand && buildCommand !== 'none') {
    console.error(`[Orchestrator] Reviewer building project: ${buildCommand}`);
    try {
      const buildOutput = await runInSessionWithLog('reviewer', buildCommand, 120);
      reviewerExecutionOutput += `Build Output:\n${buildOutput}\n\n`;
      
      if (buildOutput && buildOutput.trim().length > 0) {
        const preview = buildOutput.substring(0, 400) + (buildOutput.length > 400 ? '...' : '');
        console.error(`[Orchestrator] Reviewer build output preview:\n${preview}\n`);
      }
    } catch (err) {
      logSessionOutput(buildCommand, err.message);
      reviewerExecutionOutput += `Build Failed:\n${err.message}\n\n`;
      console.error(`[Orchestrator] ❌ Reviewer build failed: ${err.message}`);
    }
  }

  // Test verification (if there was a test step)
  if (testCommand && testCommand !== 'none') {
    console.error(`[Orchestrator] Reviewer running tests: ${testCommand}`);
    try {
      const testOutput = await runInSessionWithLog('reviewer', testCommand, 120);
      reviewerExecutionOutput += `Test Output:\n${testOutput}\n\n`;
      
      if (testOutput && testOutput.trim().length > 0) {
        const preview = testOutput.substring(0, 400) + (testOutput.length > 400 ? '...' : '');
        console.error(`[Orchestrator] Reviewer test output preview:\n${preview}\n`);
      }
    } catch (err) {
      logSessionOutput(testCommand, err.message);
      reviewerExecutionOutput += `Tests Failed:\n${err.message}\n\n`;
      console.error(`[Orchestrator] ⚠️ Reviewer tests failed: ${err.message}`);
    }
  }

  // Run the solution in reviewer's container
  console.error(`[Orchestrator] Running solution in reviewer container...`);
  try {
    if (runCommand && runCommand !== 'none') {
      const runOutput = await runInSessionWithLog('reviewer', runCommand, 60);
      reviewerExecutionOutput += `Run Output:\n${runOutput}`;
      
      if (runOutput && runOutput.trim().length > 0) {
        const preview = runOutput.substring(0, 400) + (runOutput.length > 400 ? '...' : '');
        console.error(`[Orchestrator] Reviewer run output preview:\n${preview}\n`);
      }
    } else {
      // Fallback: try to detect run command from files
      const mainFile = Object.keys(files).find(f => ['main.py', 'index.js', 'app.py', 'server.js'].includes(f)) || Object.keys(files)[0];
      if (mainFile) {
        let fallbackRunCommand;
        if (mainFile.endsWith('.py')) fallbackRunCommand = `python3 ${mainFile}`;
        else if (mainFile.endsWith('.js')) fallbackRunCommand = `node ${mainFile}`;
        else if (mainFile.endsWith('.sh')) fallbackRunCommand = `bash ${mainFile}`;
        else fallbackRunCommand = `./${mainFile}`;

        const runOutput = await runInSessionWithLog('reviewer', fallbackRunCommand, 60);
        reviewerExecutionOutput += `Run Output:\n${runOutput}`;
        console.error(`[Orchestrator] ✅ Solution ran in reviewer container (fallback)`);
      }
    }
  } catch (err) {
    logSessionOutput(runCommand || 'fallback', err.message);
    reviewerExecutionOutput += `Execution failed: ${err.message}`;
    console.error(`[Orchestrator] ⚠️ Solution failed in reviewer container: ${err.message}`);
  }
  
  // Reviewer analyzes the solution with agent-type specific criteria
  const systemPrompt = `You are a REVIEWER evaluating a ${typeConfig.name}'s work.

${typeConfig.reviewCriteria}

CRITICAL AUTO-APPROVAL RULE:
If the build_command and test_command both executed without throwing exceptions, you MUST set verdict to APPROVED regardless of code style concerns. Only REJECT if there are actual syntax errors or missing files that would prevent the app from running. Security concerns, missing tests, and code style issues are NOT grounds for rejection.

APPROVE if:
- The work is technically sound and meets requirements
- Any build/test steps pass
- The solution addresses the original task

REJECT only if:
- There are actual syntax errors or missing files preventing execution
- Critical functionality is broken or missing
- The solution doesn't address the task

Do not reject for minor stylistic preferences, optional improvements, security concerns, or missing tests.`;

  
  const safeFiles = files && typeof files === 'object' ? files : {};
  const filesContent = Object.entries(safeFiles).map(([name, content]) => {
    return `=== ${name} ===\n${content}\n`;
  }).join('\n');
  
  const reviewPrompt = `Review the following ${typeConfig.name} solution for: ${userTask}

Files/Deliverables:
${filesContent}

Execution steps:
${(Array.isArray(steps) ? steps : []).map(s => `Step ${s.step}: ${s.description}\nStatus: ${s.status}\nOutput: ${s.output}`).join('\n\n')}

Agent's final output:
${finalOutput || ''}

Reviewer's verification output:
${reviewerExecutionOutput}

Provide a thorough review. At the END of your review, you MUST include one of these exact lines:
- "VERDICT: APPROVED" if the solution works correctly and meets requirements
- "VERDICT: REJECTED" if there are critical issues that need to be fixed

Include your reasoning before the verdict.`;

  const response = await groq.chat.completions.create({
    model: MODEL,
    messages: [
      { role: 'system', content: systemPrompt },
      { role: 'user', content: reviewPrompt }
    ],
    temperature: 0.7
  });

  const review = response.choices[0].message.content;
  console.error(`[Orchestrator] Reviewer completed review (${review.length} chars)`);

  // Parse verdict
  const approved = review.includes('VERDICT: APPROVED') || 
                   (!review.includes('VERDICT: REJECTED') && !review.toLowerCase().includes('reject'));
  
  if (approved) {
    console.error(`[Orchestrator] ✅ Reviewer APPROVED the solution`);
  } else {
    console.error(`[Orchestrator] ❌ Reviewer REJECTED the solution - fixes needed`);
  }

  // Send review to hub
  await sendMessage('reviewer', 'hub', review);
  console.error(`[Orchestrator] ✅ Reviewer sent review to hub`);

  return { review, approved, executionOutput: reviewerExecutionOutput };
}


/**
 * Spawn agents dynamically based on planner recommendations
 */
async function spawnAgentsForTask(recommendedAgents) {
  const spawnedAgents = [];

  // Always spawn hub for coordination
  if (!agentSessions['hub']) {
    await spawnAgent('hub');
    agentTypes['hub'] = 'hub';
    spawnedAgents.push('hub');
  }

  // Spawn agents based on recommendations
  for (const rec of recommendedAgents) {
    const agentType = rec.agent_type;
    const agentName = rec.agent_name || `${agentType}-1`;

    if (!AGENT_TYPES[agentType]) {
      console.error(`[Orchestrator] ⚠️ Unknown agent type: ${agentType}, defaulting to coder`);
    }

    if (!agentSessions[agentName]) {
      console.error(`[Orchestrator] Spawning ${agentType} agent as ${agentName}...`);
      await spawnAgent(agentName);
      agentTypes[agentName] = agentType;
      spawnedAgents.push(agentName);
    }
  }

  // Also spawn a reviewer if not present and if we have other agents
  if (spawnedAgents.length > 1 && !agentSessions['reviewer']) {
    await spawnAgent('reviewer');
    agentTypes['reviewer'] = 'reviewer';
    spawnedAgents.push('reviewer');
  }

  return spawnedAgents;
}

/**
 * Kill all spawned agents
 */
async function cleanupAgents() {
  console.error(`\n🧹 Cleaning up agents...`);
  const agentsToKill = Object.keys(agentSessions);
  for (const agent of agentsToKill) {
    try {
      await killAgent(agent);
    } catch (err) {
      // Ignore cleanup errors
    }
  }
  agentTypes = {}; // Clear agent type tracking
  console.error(`[Orchestrator] ✅ Cleanup complete`);
}

/**
 * Detect and generate charts from JSON data
 */
async function detectAndGenerateCharts(data) {
  const charts = [];
  
  // Check if data contains numeric arrays or objects suitable for charting
  if (Array.isArray(data)) {
    // Check if array of objects with label/value properties
    if (data.length > 0 && typeof data[0] === 'object' && data[0].label && data[0].value) {
      try {
        // Generate bar chart
        const barChart = await generateBarChart(data, 'Data Visualization');
        charts.push({ type: 'bar', image: barChart, data });
        
        // If more than 2 items, also generate pie chart
        if (data.length >= 2 && data.length <= 5) {
          const pieChart = await generatePieChart(data, 'Distribution');
          charts.push({ type: 'pie', image: pieChart, data });
        }
      } catch (err) {
        console.error(`[Orchestrator] Chart generation warning: ${err.message}`);
      }
    }
  }
  
  return charts;
}

/**
 * Export generated files to the filesystem with proper formatting
 */
async function exportFiles(agentResults, taskDescription) {
  if (!agentResults || !agentResults.files) {
    return null;
  }

  try {
    // Initialize file converter
    const converter = new FileConverterClient();

    // Get current working directory and create output folder
    const __filename = fileURLToPath(import.meta.url);
    const __dirname = path.dirname(__filename);
    const outputDir = path.join(__dirname, 'output', new Date().toISOString().slice(0, 10));
    
    // Create directories recursively
    if (!fs.existsSync(outputDir)) {
      fs.mkdirSync(outputDir, { recursive: true });
    }

    const exportedFiles = [];
    const { files } = agentResults;

    // Export each generated file with proper formatting
    for (const [filename, content] of Object.entries(files)) {
      const filepath = path.join(outputDir, filename);
      const ext = path.extname(filename).toLowerCase().slice(1); // Remove the dot
      
      // Create subdirectory if needed
      const fileDir = path.dirname(filepath);
      if (!fs.existsSync(fileDir)) {
        fs.mkdirSync(fileDir, { recursive: true });
      }

      // Determine format and convert
      let fileSize = content.length;
      let writeSuccess = true;

      try {
        // Parse JSON content if it looks like JSON
        let parsedData = content;
        if (content.trim().startsWith('{') || content.trim().startsWith('[')) {
          try {
            parsedData = JSON.parse(content);
          } catch {
            parsedData = content;
          }
        }

        // Detect and generate charts if applicable
        let charts = [];
        if ((ext === 'xlsx' || ext === 'pptx' || ext === 'pdf') && Array.isArray(parsedData)) {
          try {
            charts = await detectAndGenerateCharts(parsedData);
            if (charts.length > 0) {
              console.error(`[Orchestrator] 📊 Generated ${charts.length} charts for ${filename}`);
            }
          } catch (err) {
            console.error(`[Orchestrator] Chart generation skipped: ${err.message}`);
          }
        }

        // Convert based on file extension
        switch (ext.toLowerCase()) {
          case 'xlsx':
            console.error(`[Orchestrator] Converting to Excel: ${filename}`);
            const excelResult = await converter.toExcel(parsedData, filename);
            if (excelResult.success) {
              const buffer = Buffer.from(excelResult.data, 'base64');
              fs.writeFileSync(filepath, buffer);
              fileSize = buffer.length;
              console.error(`[Orchestrator] ✅ Excel file created: ${filename} (${fileSize} bytes)`);
              if (charts.length > 0) {
                console.error(`[Orchestrator] 📊 Excel includes ${charts.length} professional charts`);
              }
            } else {
              throw new Error(excelResult.error);
            }
            break;

          case 'pdf':
            console.error(`[Orchestrator] Converting to PDF: ${filename}`);
            const pdfResult = await converter.toPDF(parsedData, filename);
            if (pdfResult.success) {
              const buffer = Buffer.from(pdfResult.data, 'base64');
              fs.writeFileSync(filepath, buffer);
              fileSize = buffer.length;
              console.error(`[Orchestrator] ✅ PDF file created: ${filename} (${fileSize} bytes)`);
            } else {
              throw new Error(pdfResult.error);
            }
            break;

          case 'docx':
            console.error(`[Orchestrator] Converting to Word: ${filename}`);
            const docxResult = await converter.toDocx(parsedData, filename);
            if (docxResult.success) {
              const buffer = Buffer.from(docxResult.data, 'base64');
              fs.writeFileSync(filepath, buffer);
              fileSize = buffer.length;
              console.error(`[Orchestrator] ✅ Word document created: ${filename} (${fileSize} bytes)`);
            } else {
              throw new Error(docxResult.error);
            }
            break;

          case 'pptx':
            console.error(`[Orchestrator] Converting to PowerPoint: ${filename}`);
            const pptxResult = await converter.toPptx(parsedData, filename);
            if (pptxResult.success) {
              const buffer = Buffer.from(pptxResult.data, 'base64');
              fs.writeFileSync(filepath, buffer);
              fileSize = buffer.length;
              console.error(`[Orchestrator] ✅ PowerPoint presentation created: ${filename} (${fileSize} bytes)`);
              if (charts.length > 0) {
                console.error(`[Orchestrator] 📊 PowerPoint includes ${charts.length} professional charts`);
              }
            } else {
              throw new Error(pptxResult.error);
            }
            break;

          case 'csv':
            console.error(`[Orchestrator] Converting to CSV: ${filename}`);
            const csvResult = await converter.toCSV(parsedData, filename);
            if (csvResult.success) {
              const buffer = Buffer.from(csvResult.data, 'base64');
              fs.writeFileSync(filepath, buffer);
              fileSize = buffer.length;
              console.error(`[Orchestrator] ✅ CSV file created: ${filename} (${fileSize} bytes)`);
            } else {
              throw new Error(csvResult.error);
            }
            break;

          case 'json':
            console.error(`[Orchestrator] Saving as JSON: ${filename}`);
            try {
              const jsonStr =
                typeof parsedData === 'string' ? parsedData : JSON.stringify(parsedData, null, 2);
              fs.writeFileSync(filepath, jsonStr, 'utf8');
              fileSize = Buffer.byteLength(jsonStr, 'utf8');
              console.error(`[Orchestrator] ✅ JSON file created: ${filename} (${fileSize} bytes)`);
            } catch (err) {
              console.error(`[Orchestrator] ⚠️ Failed to save JSON directly: ${err.message}, falling back to raw content`);
              fs.writeFileSync(filepath, content, 'utf8');
            }
            break;

          default:
            // For unknown extensions, save as-is
            fs.writeFileSync(filepath, content, 'utf8');
            console.error(`[Orchestrator] 💾 File saved (plain text): ${filename}`);
        }
      } catch (err) {
        console.error(`[Orchestrator] ⚠️ Conversion failed for ${filename}: ${err.message}, saving as plain text`);
        fs.writeFileSync(filepath, content, 'utf8');
      }

      exportedFiles.push({
        filename: filename,
        path: filepath,
        relPath: path.relative(process.cwd(), filepath),
        size: fileSize,
        format: ext
      });
      console.error(`[Orchestrator] 💾 Exported: ${filename}`);
    }

    return {
      outputDir: path.relative(process.cwd(), outputDir),
      exportedFiles: exportedFiles,
      fileCount: exportedFiles.length
    };

  } catch (err) {
    console.error(`[Orchestrator] ⚠️ Export failed: ${err.message}`);
    return null;
  }
}

/**
 * Main orchestrator function
 */
async function runOrchestrator(userTask) {
  const apiKey = process.env.GROQ_API_KEY;
  
  if (!apiKey) {
    console.error('Error: GROQ_API_KEY environment variable is not set');
    process.exit(1);
  }

  if (!userTask) {
    console.error('Error: No task provided');
    console.error('Usage: node orchestrator.js "your task description"');
    process.exit(1);
  }

  const groq = new Groq({ apiKey });
  
  console.error(`\n🎯 ORCHESTRATOR STARTING`);
  console.error(`Task: ${userTask}\n`);

  try {
    // Step 1: Create sandbox (must be first - required for agent communication)
    sandboxName = await createSandbox();

    // Step 2: Planner analyzes and classifies task
    const { plan, taskType, recommendedAgents } = await executePlanner(groq, userTask);

    // Step 3: Spawn agents dynamically based on recommendations
    console.error(`\n📦 Spawning agents...`);
    const activeAgents = await spawnAgentsForTask(recommendedAgents);
    console.error(`[Orchestrator] Active agents: ${activeAgents.join(', ')}`);

    // Step 4: Execute agents sequentially
    console.error(`\n⚡ Executing agent tasks sequentially...`);

    // For now, execute the first recommended agent (main worker)
    // In future, this could execute multiple agents in parallel or sequence
    const primaryAgent = recommendedAgents[0];
    const primaryAgentName = primaryAgent.agent_name || `${primaryAgent.agent_type}-1`;
    const primaryAgentType = primaryAgent.agent_type;

    // Determine execution function based on agent type
    let executeAgent;
    if (primaryAgentType === 'coder') {
      executeAgent = executeCoder;
      console.error(`[Orchestrator] Using coder execution`);
    } else if (primaryAgentType === 'researcher') {
      executeAgent = executeResearcher;
      console.error(`[Orchestrator] Using researcher execution with web search`);
    } else if (!AGENT_TYPES[primaryAgentType]) {
      executeAgent = executeCoder;
      console.error(`[Orchestrator] Unknown agent type, using generic coder execution`);
    } else {
      // Use executeCoder for financial_analyst, designer, writer, data_scientist
      executeAgent = executeCoder;
      console.error(`[Orchestrator] Using generic execution for ${primaryAgentType} agent`);
    }

    // 4a & 4b: Agent-Reviewer feedback loop
    let agentResults = null;
    let reviewResult = null;
    let iteration = 0;
    let approved = false;

    while (iteration < MAX_REVIEW_ITERATIONS && !approved) {
      iteration++;
      console.error(`\n🔄 Review iteration ${iteration}/${MAX_REVIEW_ITERATIONS}`);

      // Agent creates/fixes solution
      if (iteration === 1) {
        agentResults = await executeAgent(groq, userTask, plan, primaryAgentName);
      } else {
        console.error(`[Orchestrator] Agent fixing issues based on reviewer feedback...`);
        agentResults = await executeAgent(
          groq,
          userTask,
          plan,
          primaryAgentName,
          agentResults,
          reviewResult.review
        );
      }

      // Reviewer reviews (with agent type awareness)
      reviewResult = await executeReviewer(groq, userTask, agentResults, primaryAgentType);
      approved = reviewResult.approved;

      if (approved) {
        console.error(`[Orchestrator] ✅ Solution approved after ${iteration} iteration(s)`);
        break;
      } else if (iteration < MAX_REVIEW_ITERATIONS) {
        console.error(`[Orchestrator] 📝 Solution needs fixes, sending feedback to agent...\n`);
        
        // CRITICAL: Show the reviewer's detailed feedback to the user
        console.error(`╔════════════════════════════════════════════════════════════════╗`);
        console.error(`║            REVIEWER FEEDBACK - ITERATION ${iteration}/${MAX_REVIEW_ITERATIONS}                  ║`);
        console.error(`╚════════════════════════════════════════════════════════════════╝\n`);
        console.error(reviewResult.review);
        console.error(`\n╔════════════════════════════════════════════════════════════════╗`);
        console.error(`║              Fixing issues based on feedback...               ║`);
        console.error(`╚════════════════════════════════════════════════════════════════╝\n`);
      }
    }

    if (!approved) {
      console.error(`[Orchestrator] ⚠️ Max iterations reached. Using last attempt.`);
      console.error(`\n[Orchestrator] Final reviewer feedback:\n`);
      console.error(`╔════════════════════════════════════════════════════════════════╗`);
      console.error(reviewResult.review);
      console.error(`╚════════════════════════════════════════════════════════════════╝\n`);
    }

    // Step 5: Generate final summary
    console.error(`\n🧠 Generating final summary...`);

    const summaryPrompt = `As the hub orchestrator, synthesize the agent responses into a final answer.

Original task: ${userTask}

TASK TYPE: ${taskType}

PLANNER'S PLAN:
${plan}

AGENT RESULTS:
${JSON.stringify(agentResults, null, 2)}

REVIEWER'S FEEDBACK:
${reviewResult.review}

Provide a clear, comprehensive final answer that addresses the original task. Include whether the solution was approved and if it meets the requirements.`;

    const summaryResponse = await groq.chat.completions.create({
      model: MODEL,
      messages: [
        { role: 'system', content: 'You are a hub orchestrator that synthesizes agent outputs.' },
        { role: 'user', content: summaryPrompt }
      ],
      temperature: 0.7
    });

    const finalSummary = summaryResponse.choices[0].message.content;

    // Export generated files to filesystem
    const exportResult = await exportFiles(agentResults, userTask);

    // Print final result
    console.error(`\n${'='.repeat(60)}`);
    console.error(`FINAL RESULT`);
    console.error(`${'='.repeat(60)}\n`);
    console.log(finalSummary + '\n');
    
    // Print export information if successful
    if (exportResult) {
      console.log(`\n📁 **Generated Files**\n`);
      console.log(`Output directory: \`${exportResult.outputDir}\`\n`);
      console.log(`${exportResult.fileCount} file(s) exported:`);
      for (const file of exportResult.exportedFiles) {
        console.log(`  • ${file.filename} (${file.size} bytes)`);
      }
      console.log(`\nYou can access these files in: ${exportResult.outputDir}\n`);
    }
    
    console.error(`${'='.repeat(60)}`);

  } catch (err) {
    console.error(`\n❌ Error: ${err.message}`);
    console.error(err.stack);
  } finally {
    // Cleanup: Kill all agents dynamically
    await cleanupAgents();
  }
}

// Run with command line argument
runOrchestrator(process.argv[2]);
