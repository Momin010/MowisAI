#!/usr/bin/env node

import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js';
import net from 'net';
import { v4 as uuidv4 } from 'uuid';

// Socket path for the MowisAI Engine
const SOCKET_PATH = '/tmp/mowisai.sock';

// Default resource limits
const DEFAULT_TIMEOUT = 30;
const DEFAULT_MEMORY_MB = 512;
const DEFAULT_CPU_PERCENT = 50;

/**
 * Send a command to the MowisAI Engine via Unix socket
 * @param {string} command - The shell command to execute
 * @param {number} timeoutSecs - Timeout in seconds
 * @returns {Promise<string>} - The output from the engine
 */
async function sendToEngine(command, timeoutSecs = DEFAULT_TIMEOUT) {
  return new Promise((resolve, reject) => {
    const taskId = uuidv4();
    const request = {
      request_type: 'exec',
      task_id: taskId,
      command: command,
      timeout_secs: timeoutSecs,
      memory_mb: DEFAULT_MEMORY_MB,
      cpu_percent: DEFAULT_CPU_PERCENT
    };

    const client = net.createConnection(SOCKET_PATH, () => {
      console.error(`[MowisAI Bridge] Sending command: ${command}`);
      
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
          console.error(`[MowisAI Bridge] Response status: ${response.status}`);
          
          if (response.status === 'error') {
            resolve(`Error: ${response.output}`);
          } else {
            resolve(response.output);
          }
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
    }, (timeoutSecs + 5) * 1000);
  });
}


// Create the MCP server
const server = new Server(
  {
    name: 'MowisAI Bridge',
    version: '0.1.0',
  },
  {
    capabilities: {
      tools: {},
    },
  }
);

// List available tools
server.setRequestHandler(ListToolsRequestSchema, async () => {
  return {
    tools: [
      {
        name: 'shell_exec',
        description: 'Execute a shell command in the MowisAI sandbox container',
        inputSchema: {
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
      {
        name: 'file_read',
        description: 'Read the contents of a file from the sandbox',
        inputSchema: {
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
      {
        name: 'file_write',
        description: 'Write content to a file in the sandbox',
        inputSchema: {
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
      {
        name: 'file_list',
        description: 'List files and directories in the sandbox',
        inputSchema: {
          type: 'object',
          properties: {
            path: {
              type: 'string',
              description: 'The path to list (default: current directory)',
            },
          },
        },
      },
    ],
  };
});

// Handle tool calls
server.setRequestHandler(CallToolRequestSchema, async (request) => {
  const { name, arguments: args } = request.params;

  try {
    switch (name) {
      case 'shell_exec': {
        const { command, timeout_secs } = args;
        const output = await sendToEngine(command, timeout_secs);
        return {
          content: [
            {
              type: 'text',
              text: output,
            },
          ],
        };
      }

      case 'file_read': {
        const { path } = args;
        const command = `cat '${path.replace(/'/g, "'\\''")}'`;
        const output = await sendToEngine(command);
        return {
          content: [
            {
              type: 'text',
              text: output,
            },
          ],
        };
      }

      case 'file_write': {
        const { path, content } = args;
        // Escape single quotes in content for shell safety
        const escapedContent = content.replace(/'/g, "'\\''");
        const command = `echo '${escapedContent}' > '${path.replace(/'/g, "'\\''")}'`;
        const output = await sendToEngine(command);
        return {
          content: [
            {
              type: 'text',
              text: output || `File written successfully: ${path}`,
            },
          ],
        };
      }

      case 'file_list': {
        const path = args.path || '.';
        const command = `ls -la '${path.replace(/'/g, "'\\''")}'`;
        const output = await sendToEngine(command);
        return {
          content: [
            {
              type: 'text',
              text: output,
            },
          ],
        };
      }

      default:
        throw new Error(`Unknown tool: ${name}`);
    }
  } catch (err) {
    return {
      content: [
        {
          type: 'text',
          text: `Error: ${err.message}`,
        },
      ],
      isError: true,
    };
  }
});

// Start the server with stdio transport
async function main() {
  const transport = new StdioServerTransport();
  console.error('MowisAI Bridge v0.1.0 starting...');
  console.error(`Connecting to engine at: ${SOCKET_PATH}`);
  
  await server.connect(transport);
  
  console.error('MowisAI Bridge running on stdio');
}

main().catch((error) => {
  console.error('Fatal error:', error);
  process.exit(1);
});
