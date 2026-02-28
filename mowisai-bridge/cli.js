#!/usr/bin/env node

import { spawn } from 'child_process';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';
import chalk from 'chalk';
import boxen from 'boxen';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

function printHeader() {
  console.log(boxen(
    chalk.cyan.bold('MowisAI') + chalk.gray('  //  v0.1') + '\n' + chalk.gray('AI Agent Sandbox Platform'),
    { padding: 1, borderStyle: 'round', borderColor: 'cyan', textAlignment: 'center' }
  ));
}

function formatStatusLine(line) {
  line = line.trim();
  if (!line) return;

  // Skip raw engine socket lines
  if (/^(🔗|📨|▶️|✅|👋|🏖️|🤖|💀|🔍|📡|❌)/.test(line)) return;

  // Section headers
  if (line.startsWith('🎯')) return console.log('\n' + chalk.cyan.bold(line));
  if (line.startsWith('📦')) return console.log('\n' + chalk.yellow(line));
  if (line.startsWith('📋')) return console.log('\n' + chalk.yellow(line));
  if (line.startsWith('📤')) return console.log('\n' + chalk.yellow(line));
  if (line.startsWith('⚡')) return console.log('\n' + chalk.yellow(line));
  if (line.startsWith('📨')) return console.log('\n' + chalk.yellow(line));
  if (line.startsWith('🧠')) return console.log('\n' + chalk.magenta(line));
  if (line.startsWith('🧹')) return console.log('\n' + chalk.gray(line));
  if (line.startsWith('=')) return; // skip separator lines from orchestrator

  // Orchestrator log lines
  if (line.includes('[Orchestrator]')) {
    const clean = line.replace('[Orchestrator] ', '');
    if (clean.startsWith('✅')) return console.log('  ' + chalk.green(clean));
    if (clean.startsWith('❌') || clean.toLowerCase().includes('error')) return console.log('  ' + chalk.red(clean));
    if (clean.startsWith('⚠')) return console.log('  ' + chalk.yellow(clean));
    if (clean.startsWith('→') || clean.startsWith('←')) return; // skip socket ops
    return console.log('  ' + chalk.gray(clean));
  }

  // Plan content lines (between 📋 and next section)
  console.log('  ' + chalk.white(line));
}

// Spinner for showing progress
function startSpinner(text) {
  const frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
  let i = 0;
  const timer = setInterval(() => {
    process.stdout.write(`\r  ${chalk.cyan(frames[i % frames.length])} ${chalk.gray(text)}`);
    i++;
  }, 80);
  return {
    update: (newText) => { text = newText; },
    stop: () => {
      clearInterval(timer);
      process.stdout.write('\r' + ' '.repeat(70) + '\r');
    }
  };
}


async function run(task) {
  if (!task) {
    console.log(chalk.red('Error: No task provided'));
    console.log('Usage: ' + chalk.cyan('node cli.js run "your task here"'));
    process.exit(1);
  }

  printHeader();
  console.log(chalk.gray('Task: ') + chalk.white.bold(task) + '\n');

  const spinner = startSpinner('Initializing agents...');

  return new Promise((resolve, reject) => {
    const child = spawn('node', [join(__dirname, 'orchestrator.js'), task], {
      env: process.env,
      stdio: ['ignore', 'pipe', 'pipe']
    });

    let finalOutput = '';

    // stdout = only the final result (console.log in orchestrator)
    child.stdout.on('data', (data) => {
      finalOutput += data.toString();
    });

    // stderr = all the status/progress logs (console.error in orchestrator)
    child.stderr.on('data', (data) => {
      const lines = data.toString().split('\n');
      lines.forEach(line => {
        // Update spinner based on progress
        if (line.includes('Planning')) spinner.update('Planning task...');
        else if (line.includes('Spawning')) spinner.update('Spawning agents...');
        else if (line.includes('Executing')) spinner.update('Agents working in parallel...');
        else if (line.includes('Generating final')) spinner.update('Generating final summary...');
        else if (line.includes('Cleaning')) spinner.update('Cleaning up...');
        formatStatusLine(line);
      });
    });

    child.on('close', (code) => {
      spinner.stop();
      
      if (finalOutput.trim()) {
        console.log('\n' + boxen(
          chalk.white(finalOutput.trim()),
          {
            padding: 1,
            borderStyle: 'double',
            borderColor: 'green',
            title: chalk.green.bold('✓ Result'),
            titleAlignment: 'left'
          }
        ));
      }

      if (code !== 0 && !finalOutput.trim()) {
        console.log(chalk.red('\nAgent exited with errors.'));
      }

      resolve();
    });

    child.on('error', (err) => {
      spinner.stop();
      reject(err);
    });
  });
}


// Entry point
const [,, command, ...args] = process.argv;

if (command === 'run') {
  run(args.join(' '));
} else {
  printHeader();
  console.log(chalk.gray('Commands:'));
  console.log('  ' + chalk.cyan('node cli.js run "task"') + chalk.gray('  — run an agent task'));
}
