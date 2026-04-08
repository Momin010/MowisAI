#!/usr/bin/env node
const { tool, pass, fail, skip, section } = require('./common');

async function testUtils() {
  section('🛠️  UTILS (echo & spawn)');

  const e = await tool('echo', { message: 'hello' });
  console.log('echo tool result', JSON.stringify(e));
  e.status === 'ok' && e.result?.echo?.includes('hello') ? pass('echo tool') : fail('echo tool', JSON.stringify(e));

  const sa = await tool('spawn_agent', { task: 'do nothing', tools: [] });
  console.log('spawn_agent result', JSON.stringify(sa));
  sa.status === 'ok' && sa.result?.success ? pass('spawn_agent tool') : fail('spawn_agent tool', JSON.stringify(sa));
}

module.exports = testUtils;
