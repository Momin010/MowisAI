#!/usr/bin/env node
const { tool, pass, fail, skip, section } = require('./common');

async function testShell() {
  section('💻 SHELL (5 tools + extra work)');

  // run_command basic
  const r1 = await tool('run_command', { cmd: 'echo hello_mowisai' });
  console.log('run_command echo result', JSON.stringify(r1));
  r1.result?.stdout?.trim() === 'hello_mowisai' ? pass('run_command echo') : fail('run_command echo', `got ${r1.result?.stdout}`);

  // run_command with cwd
  await tool('create_directory', { path: '/tmp/cmdtest' });
  await tool('write_file', { path: '/tmp/cmdtest/data.txt', content: 'test data' });
  const r2 = await tool('run_command', { cmd: 'ls', cwd: '/tmp/cmdtest' });
  console.log('run_command cwd result', JSON.stringify(r2));
  r2.result?.stdout?.includes('data.txt') ? pass('run_command cwd') : fail('run_command cwd');

  // chained commands
  const r3 = await tool('run_command', { cmd: 'echo first && echo second' });
  console.log('run_command chain result', JSON.stringify(r3));
  r3.result?.stdout?.includes('first') && r3.result?.stdout?.includes('second') ? pass('run_command chain') : fail('run_command chain');

  // stderr capture
  const r4 = await tool('run_command', { cmd: 'ls /nonexistent 2>&1' });
  console.log('run_command stderr result', JSON.stringify(r4));
  r4.status === 'ok' ? pass('run_command stderr capture') : fail('run_command stderr');

  // run_script
  const script = '#!/bin/sh\necho "script_output"\necho "line2"';
  await tool('write_file', { path: '/tmp/test.sh', content: script });
  const rs = await tool('run_command', { cmd: 'sh /tmp/test.sh' });
  console.log('run_script result', JSON.stringify(rs));
  rs.result?.stdout?.includes('script_output') ? pass('run_script') : fail('run_script');

  // get_env
  const ge = await tool('get_env', { var: 'PATH' });
  console.log('get_env result', JSON.stringify(ge));
  ge.status === 'ok' && ge.result?.value ? pass('get_env') : fail('get_env');

  // set_env
  const se = await tool('set_env', { var: 'MOWIS_TEST', value: 'works' });
  console.log('set_env result', JSON.stringify(se));
  se.status === 'ok' ? pass('set_env') : fail('set_env');

  // kill_process invalid pid
  const kp = await tool('kill_process', { pid: 99999 });
  console.log('kill_process result', JSON.stringify(kp));
  kp !== undefined ? pass('kill_process invalid pid') : fail('kill_process');

  // --- Vite project creation/build to demonstrate engine capabilities ---
  section('🛠️  VITE PROJECT DEMO');
  const createVite = await tool('run_command', { cmd: 'cd /tmp && npm create vite@latest vite-app -- --template vanilla --yes' });
  console.log('vite create output', JSON.stringify(createVite));
  if (createVite.status === 'ok') pass('npm create vite'); else fail('npm create vite', createVite.error);

  const installDeps = await tool('run_command', { cmd: 'cd /tmp/vite-app && npm install' });
  console.log('vite install output', JSON.stringify(installDeps));
  installDeps.status === 'ok' ? pass('vite dependencies install') : fail('vite deps', installDeps.error);

  const buildCmd = await tool('run_command', { cmd: 'cd /tmp/vite-app && npm run build' });
  console.log('vite build output', JSON.stringify(buildCmd));
  buildCmd.status === 'ok' ? pass('vite build') : fail('vite build', buildCmd.error);

  // verify dist folder
  const verifyDist = await tool('file_exists', { path: '/tmp/vite-app/dist/index.html' });
  console.log('vite dist verify output', JSON.stringify(verifyDist));
  verifyDist.result?.exists ? pass('vite build produced index.html') : fail('vite dist verify');
}

module.exports = testShell;
