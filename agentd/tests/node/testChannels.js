#!/usr/bin/env node
const { tool, pass, fail, skip, section } = require('./common');

async function testChannels() {
  section('🤝 AGENT COORDINATION (6 tools)');

  const cc = await tool('create_channel', { name: 'orchestrator' });
  console.log('create_channel result', JSON.stringify(cc));
  cc.status === 'ok' ? pass('create_channel') : fail('create_channel', cc.error);

  const sm = await tool('send_message', { channel: 'orchestrator', message: 'task: build', sender: 'hub' });
  console.log('send_message result', JSON.stringify(sm));
  sm.status === 'ok' ? pass('send_message') : fail('send_message', sm.error);

  await tool('send_message', { channel: 'orchestrator', message: 'task: test', sender: 'hub' });
  await tool('send_message', { channel: 'orchestrator', message: 'task: deploy', sender: 'hub' });

  const rm = await tool('read_messages', { channel: 'orchestrator' });
  console.log('read_messages result', JSON.stringify(rm));
  if (rm.status === 'ok') {
    const msgs = rm.result?.messages || rm.result;
    (Array.isArray(msgs) && msgs.length > 0) ? pass('read_messages') : fail('read_messages empty');
  } else fail('read_messages', rm.error);

  const bc = await tool('broadcast', { message: 'all start', sender: 'hub' });
  console.log('broadcast result', JSON.stringify(bc));
  bc.status === 'ok' ? pass('broadcast') : fail('broadcast', bc.error);

  const sa = await tool('spawn_agent', { task: 'echo hello', tools: ['run_command'] });
  console.log('spawn_agent result', JSON.stringify(sa));
  sa.status === 'ok' ? pass('spawn_agent') : fail('spawn_agent', sa.error);

  const wf = await tool('wait_for', { channel: 'orchestrator', timeout: 100 });
  console.log('wait_for result', JSON.stringify(wf));
  wf !== undefined ? pass('wait_for handled') : fail('wait_for');
}

module.exports = testChannels;
