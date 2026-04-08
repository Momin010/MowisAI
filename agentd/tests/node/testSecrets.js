#!/usr/bin/env node
const { tool, pass, fail, skip, section } = require('./common');

async function testSecrets() {
  section('🔐 SECRETS (2 tools)');

  const ss = await tool('secret_set', { name: 'API_KEY', value: 'sk-test-123' });
  console.log('secret_set result', JSON.stringify(ss));
  ss.status === 'ok' ? pass('secret_set') : fail('secret_set', ss.error);

  const sg = await tool('secret_get', { name: 'API_KEY' });
  console.log('secret_get result', JSON.stringify(sg));
  sg.status === 'ok' && sg.result?.value === 'sk-test-123' ? pass('secret_get') : fail('secret_get', JSON.stringify(sg));
}

module.exports = testSecrets;
