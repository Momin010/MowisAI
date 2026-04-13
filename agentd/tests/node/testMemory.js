#!/usr/bin/env node
const { tool, pass, fail, skip, section } = require('./common');

async function testMemory() {
  section('🧠 MEMORY / STATE (6 tools)');

  const ms = await tool('memory_set', { key: 'agent_goal', value: 'build pipeline' });
  console.log('memory_set result', JSON.stringify(ms));
  ms.status === 'ok' ? pass('memory_set') : fail('memory_set', ms.error);

  await tool('memory_set', { key: 'agent_status', value: 'running' });
  await tool('memory_set', { key: 'task_count', value: '42' });

  const mg = await tool('memory_get', { key: 'agent_goal' });
  console.log('memory_get result', JSON.stringify(mg));
  mg.status === 'ok' && mg.result?.value === 'build pipeline' ? pass('memory_get') : fail('memory_get', JSON.stringify(mg));

  const ml = await tool('memory_list', {});
  console.log('memory_list result', JSON.stringify(ml));
  ml.status === 'ok' && ml.result?.keys?.includes('agent_goal') ? pass('memory_list') : fail('memory_list', ml.error);

  const msave = await tool('memory_save', { path: '/tmp/memory.json' });
  console.log('memory_save result', JSON.stringify(msave));
  msave.status === 'ok' ? pass('memory_save') : fail('memory_save', msave.error);

  const md = await tool('memory_delete', { key: 'agent_goal' });
  console.log('memory_delete result', JSON.stringify(md));
  md.status === 'ok' ? pass('memory_delete') : fail('memory_delete', md.error);

  const gone = await tool('memory_get', { key: 'agent_goal' });
  gone.status !== 'ok' || gone.result?.value === null ? pass('memory_delete verify') : fail('memory_delete verify', JSON.stringify(gone));

  const mload = await tool('memory_load', { path: '/tmp/memory.json' });
  console.log('memory_load result', JSON.stringify(mload));
  mload.status === 'ok' ? pass('memory_load') : fail('memory_load', mload.error);

  const restored = await tool('memory_get', { key: 'agent_goal' });
  restored.result?.value === 'build pipeline' ? pass('memory_load verify') : fail('memory_load verify', JSON.stringify(restored));
}

module.exports = testMemory;
