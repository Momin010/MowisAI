#!/usr/bin/env node
const { tool, pass, fail, skip, section } = require('./common');

async function testWeb() {
  section('🔍 SEARCH / WEB (3 tools)');

  const ws = await tool('web_search', { query: 'MowisAI autonomous agent infrastructure' });
  console.log('web_search result', JSON.stringify(ws));
  ws.status === 'ok' ? pass('web_search') : fail('web_search', ws.error);

  const wf = await tool('web_fetch', { url: 'https://httpbin.org/html' });
  console.log('web_fetch result', JSON.stringify(wf));
  wf.status === 'ok' ? pass('web_fetch') : fail('web_fetch', wf.error);

  const wsc = await tool('web_screenshot', { url: 'https://httpbin.org', output: '/tmp/screenshot.png' });
  console.log('web_screenshot result', JSON.stringify(wsc));
  wsc.status === 'ok' ? pass('web_screenshot') : skip('web_screenshot', wsc.error);
}

module.exports = testWeb;
