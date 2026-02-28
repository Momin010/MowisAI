#!/usr/bin/env node

import Groq from 'groq-sdk';
import net from 'net';
import { v4 as uuidv4 } from 'uuid';

// Socket path for the MowisAI Engine
const SOCKET_PATH = '/tmp/mowisai.sock';

// Groq model configuration
const MODEL = 'llama-3.3-70b-versatile';

// Default resource limits for engine calls
const DEFAULT_TIMEOUT = 30;
const DEFAULT_MEMORY_MB = 512;
const DEFAULT_CPU_PERCENT = 50;

// Session management state
let sessionId = null;
let sandboxName = null;

// Define the 4 MCP tools as OpenAI-compatible tool definitions
const tools = [
  {
    type: 'function',
    function: {
      name: 'shell_exec',
      description: 'Execute a shell command in the MowisAI sandbox container',
      parameters: {
        type: 'object',
        properties: {
          command: {
            type: 'string',
            description: 'The shell command to execute',
          },
          timeout_secs: {
            type: 'number',
            description: 'Timeout in seconds (default: 30)',
          },
        },
        required: ['command'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'file_read',
      description: 'Read the contents of a file from the sandbox',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'The path to the file to read',
          },
        },
        required: ['path'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'file_write',
      description: 'Write content to a file in the sandbox',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'The path to the file to write',
          },
          content: {
            type: 'string',
            description: 'The content to write to the file',
          },
        },
        required: ['path', 'content'],
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'file_list',
      description: 'List files and directories in the sandbox',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'The path to list (default: current directory)',
          },
        },
      },
    },
  },
];

/**
 * Send a request to the MowisAI Engine via Unix socket
 * @param {Object} request - The request object to send
 * @returns {Promise<Object>} - The parsed response from the engine
 */
async function sendEngineRequest(request) {
  return new Promise((resolve, reject) => {
    const client = net.createConnection(SOCKET_PATH, () => {
      console.error(`[Agent] Sending request: ${request.request_type}`);
      
      // Send the request as newline-terminated JSON
      client.write(JSON.stringify(request) + '\n');
    });

    let responseData = '';

    client.on('data', (data) => {
      responseData += data.toString();
      
      // Check if we have a complete line (newline-terminated)
      const newlineIndex = responseData.indexOf('\n');
      if (newlineIndex !== -1) {
        // We have a complete response, close the socket
        client.end();
        
        // Extract the first line (the JSON response)
        const jsonLine = responseData.substring(0, newlineIndex).trim();
        
        try {
          const response = JSON.parse(jsonLine);
          console.error(`[Agent] Response status: ${response.status}`);
          resolve(response);
        } catch (err) {
          reject(new Error(`Failed to parse response: ${err.message}`));
        }
      }
    });

    client.on('error', (err) => {
      reject(new Error(`Socket error: ${err.message}. Is the MowisAI Engine running?`));
    });

    client.on('close', () => {
      // Socket closed, if we haven't resolved yet, something went wrong
      if (responseData.indexOf('\n') === -1 && responseData.length > 0) {
        reject(new Error('Socket closed without receiving complete response'));
      }
    });

    // Set a client-side timeout
    setTimeout(() => {
      client.destroy();
      reject(new Error('Connection timeout'));
    }, 60000); // 60 second timeout for all requests
  });
}

/**
 * Create a sandbox for the agent
 * @returns {Promise<string>} - The sandbox name
 */
async function createSandbox() {
  const sandboxId = `sandbox-${uuidv4()}`;
  console.error(`[Agent] Creating sandbox: ${sandboxId}`);
  
  const request = {
    request_type: 'create_sandbox',
    task_id: uuidv4(),
    sandbox_name: sandboxId
  };
  
  const response = await sendEngineRequest(request);
  
  if (response.status === 'error') {
    throw new Error(`Failed to create sandbox: ${response.output}`);
  }
  
  console.error(`[Agent] Sandbox created: ${sandboxId}`);
  return sandboxId;
}

/**
 * Join a sandbox with a persistent session
 * @param {string} sandboxId - The sandbox name
 * @returns {Promise<string>} - The session ID
 */
async function joinSandbox(sandboxId) {
  console.error(`[Agent] Joining sandbox ${sandboxId} as main-agent`);
  
  const request = {
    request_type: 'join_sandbox',
    task_id: uuidv4(),
    sandbox_name: sandboxId,
    agent_name: 'main-agent',
    memory_mb: DEFAULT_MEMORY_MB,
    cpu_percent: DEFAULT_CPU_PERCENT
  };
  
  const response = await sendEngineRequest(request);
  
  if (response.status === 'error') {
    throw new Error(`Failed to join sandbox: ${response.output}`);
  }
  
  const sessionId = response.session_id;
  console.error(`[Agent] Joined sandbox with session: ${sessionId}`);
  return sessionId;
}

/**
 * Kill the persistent session
 * @param {string} sessionIdToKill - The session ID to kill
 */
async function killSession(sessionIdToKill) {
  console.error(`[Agent] Killing session: ${sessionIdToKill}`);
  
  const request = {
    request_type: 'kill_session',
    task_id: uuidv4(),
    session_id: sessionIdToKill
  };
  
  try {
    const response = await sendEngineRequest(request);
    
    if (response.status === 'error') {
      console.error(`[Agent] Warning: Failed to kill session: ${response.output}`);
    } else {
      console.error(`[Agent] Session killed successfully`);
    }
  } catch (err) {
    console.error(`[Agent] Warning: Error killing session: ${err.message}`);
  }
}

/**
 * Execute a command in the persistent session
 * @param {string} command - The shell command to execute
 * @param {number} timeoutSecs - Timeout in seconds
 * @returns {Promise<string>} - The output from the engine
 */
async function callEngine(command, timeoutSecs = DEFAULT_TIMEOUT) {
  if (!sessionId) {
    throw new Error('No active session. Call joinSandbox first.');
  }
  
  const request = {
    request_type: 'run_in_session',
    task_id: uuidv4(),
    session_id: sessionId,
    command: command,
    timeout_secs: timeoutSecs
  };

  const response = await sendEngineRequest(request);
  
  if (response.status === 'error') {
    return `Error: ${response.output}`;
  }
  
  return response.output;
}

/**
 * Parse function calls from text content (for LLMs that return functions as text)
 * @param {string} content - The message content
 * @returns {Array|null} - Array of parsed tool calls or null
 */
function parseFunctionCallsFromContent(content) {
  if (!content) return null;
  
  const toolCalls = [];
  
  // Match JSON function call format: {"type": "function", "name": "...", "parameters": {...}}
  const functionRegex = /\{"type"\s*:\s*"function"[^}]*"name"\s*:\s*"([^"]*)"[^}]*"parameters"\s*:\s*(\{[^}]*\})\s*\}/g;
  
  let match;
  while ((match = functionRegex.exec(content)) !== null) {
    try {
      const name = match[1];
      const paramsJson = match[2];
      const parameters = JSON.parse(paramsJson);
      
      toolCalls.push({
        id: `call-${uuidv4()}`,
        type: 'function',
        function: {
          name: name,
          arguments: JSON.stringify(parameters)
        }
      });
    } catch (err) {
      console.error(`[Agent] Failed to parse function call: ${err.message}`);
    }
  }
  
  return toolCalls.length > 0 ? toolCalls : null;
}

/**
 * Execute a tool call by mapping it to the appropriate engine command
 * @param {Object} toolCall - The tool call from the LLM
 * @returns {Promise<string>} - The result of the tool execution
 */
async function executeToolCall(toolCall) {
  const { name, arguments: argsString } = toolCall.function;
  const args = JSON.parse(argsString);

  console.error(`[Agent] Executing tool: ${name}`);

  switch (name) {
    case 'shell_exec': {
      const { command, timeout_secs } = args;
      return await callEngine(command, timeout_secs);
    }

    case 'file_read': {
      const { path } = args;
      const command = `cat '${path.replace(/'/g, "'\\''")}'`;
      return await callEngine(command);
    }

    case 'file_write': {
      const { path, content } = args;
      // Escape single quotes in content for shell safety
      const escapedContent = content.replace(/'/g, "'\\''");
      const command = `echo '${escapedContent}' > '${path.replace(/'/g, "'\\''")}'`;
      const output = await callEngine(command);
      return output || `File written successfully: ${path}`;
    }

    case 'file_list': {
      const path = args.path || '.';
      const command = `ls -la '${path.replace(/'/g, "'\\''")}'`;
      return await callEngine(command);
    }

    default:
      throw new Error(`Unknown tool: ${name}`);
  }
}


/**
 * Main function to run the agent with a task
 * @param {string} task - The task description for the LLM
 */
async function runAgent(task) {
  const apiKey = process.env.GROQ_API_KEY;
  
  if (!apiKey) {
    console.error('Error: GROQ_API_KEY environment variable is not set');
    console.error('Please set it with: export GROQ_API_KEY=your_api_key');
    process.exit(1);
  }

  if (!task) {
    console.error('Error: No task provided');
    console.error('Usage: node agent.js "your task description"');
    process.exit(1);
  }

  // Initialize Groq client
  const groq = new Groq({ apiKey });

  console.error(`[Agent] Starting task: ${task}`);
  console.error(`[Agent] Using model: ${MODEL}`);

  // Setup session management
  try {
    // Step 1: Create sandbox
    sandboxName = await createSandbox();
    
    // Step 2: Join sandbox to get persistent session
    sessionId = await joinSandbox(sandboxName);
    
    console.error(`[Agent] Session ready: ${sessionId}`);
  } catch (err) {
    console.error(`[Agent] Failed to setup session: ${err.message}`);
    process.exit(1);
  }

  // Initialize conversation with system message and user task
  const messages = [
    {
      role: 'system',
      content: 'You are a helpful AI agent with access to a sandbox environment. You MUST use the provided tools via proper OpenAI function calling format. When you want to use a tool, respond with a tool_calls array containing the function name and arguments as valid JSON. NEVER use XML tags like <function> or write function calls as plain text. Always use the JSON tool calling format.',
    },
    {
      role: 'user',
      content: task,
    },
  ];



  // Main loop: keep calling the LLM until we get a text response (no tool calls)
  try {
    while (true) {
      console.error('[Agent] Sending request to Groq...');

      const completion = await groq.chat.completions.create({
        model: MODEL,
        messages: messages,
        tools: tools,
        tool_choice: 'auto',
      });

      const assistantMessage = completion.choices[0].message;

      // Check if there are tool_calls in the proper format
      let toolCalls = assistantMessage.tool_calls;
      
      // If no tool_calls but content exists, try to parse function calls from content
      if (!toolCalls && assistantMessage.content) {
        toolCalls = parseFunctionCallsFromContent(assistantMessage.content);
        if (toolCalls) {
          console.error(`[Agent] Parsed ${toolCalls.length} function call(s) from content`);
        }
      }

      // Add the assistant's message to the conversation
      messages.push(assistantMessage);

      // Check if there are tool calls to execute
      if (toolCalls && toolCalls.length > 0) {
        console.error(`[Agent] LLM requested ${toolCalls.length} tool call(s)`);

        // Execute each tool call and collect results
        for (const toolCall of toolCalls) {
          try {
            const result = await executeToolCall(toolCall);
            
            // Add the tool result to the conversation
            messages.push({
              role: 'tool',
              tool_call_id: toolCall.id,
              content: result,
            });
            
            console.error(`[Agent] Tool result: ${result.substring(0, 100)}${result.length > 100 ? '...' : ''}`);
          } catch (err) {
            console.error(`[Agent] Tool execution failed: ${err.message}`);
            
            // Add error as tool result
            messages.push({
              role: 'tool',
              tool_call_id: toolCall.id,
              content: `Error: ${err.message}`,
            });
          }
        }
        
        // Continue the loop to send results back to LLM
        continue;
      }

      // No tool calls - we have the final response
      if (assistantMessage.content) {
        console.log('\n' + assistantMessage.content);
        console.error('[Agent] Task completed');
        break;
      }

    }
  } catch (err) {
    console.error(`[Agent] Error: ${err.message}`);
  } finally {
    // Cleanup: Kill the session
    if (sessionId) {
      await killSession(sessionId);
    }
  }
}

// Run the agent with the task from command line arguments
runAgent(process.argv[2]);
