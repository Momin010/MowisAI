#!/usr/bin/env node
const { tool, pass, fail, skip, section } = require('./common');

async function testDevTools() {
  section('📊 CODE ANALYSIS / DEV TOOLS (4 tools)');

  // prepare small scripts
  await tool('write_file', { path: '/tmp/app.js', content: 'const x = 1; console.log(x);' });
  await tool('write_file', { path: '/tmp/app.py', content: 'x = 1\nprint(x)\n' });

  const build = await tool('build', { path: '/tmp', command: 'echo build_ok' });
  console.log('build result', JSON.stringify(build));
  build.status === 'ok' ? pass('build') : fail('build', build.error);

  const test = await tool('test', { path: '/tmp', framework: 'echo' });
  console.log('test result', JSON.stringify(test));
  test.status === 'ok' ? pass('test') : fail('test', test.error);

  const lint = await tool('lint', { path: '/tmp/app.js', language: 'js' });
  console.log('lint result', JSON.stringify(lint));
  lint.status === 'ok' ? pass('lint') : skip('lint', lint.error);

  const typecheck = await tool('type_check', { path: '/tmp' });
  console.log('type_check result', JSON.stringify(typecheck));
  typecheck.status === 'ok' ? pass('type_check') : skip('type_check', typecheck.error);
}

module.exports = testDevTools;
