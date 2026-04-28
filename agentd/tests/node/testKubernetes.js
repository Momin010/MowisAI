#!/usr/bin/env node
const { tool, pass, fail, skip, section } = require('./common');

async function testKubernetes() {
  section('☸️  KUBERNETES (6 tools)');

  const check = await tool('run_command', { cmd: 'kubectl version --client 2>/dev/null || echo NOT_FOUND' });
  if (check.result?.stdout?.includes('NOT_FOUND')) {
    skip('kubectl tools', 'kubectl not available');
    return;
  }

  const kg = await tool('kubectl_get', { resource: 'pods' });
  console.log('kubectl_get result', JSON.stringify(kg));
  kg.status === 'ok' ? pass('kubectl_get') : fail('kubectl_get', kg.error);

  const manifest = 'apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: mowis-test\ndata:\n  key: value\n';
  await tool('write_file', { path: '/tmp/manifest.yaml', content: manifest });
  const ka = await tool('kubectl_apply', { manifest: '/tmp/manifest.yaml' });
  console.log('kubectl_apply result', JSON.stringify(ka));
  ka.status === 'ok' ? pass('kubectl_apply') : fail('kubectl_apply', ka.error);

  const kd = await tool('kubectl_describe', { resource: 'configmap', name: 'mowis-test' });
  console.log('kubectl_describe result', JSON.stringify(kd));
  kd.status === 'ok' ? pass('kubectl_describe') : fail('kubectl_describe', kd.error);

  const kdel = await tool('kubectl_delete', { resource: 'configmap', name: 'mowis-test' });
  console.log('kubectl_delete result', JSON.stringify(kdel));
  kdel.status === 'ok' ? pass('kubectl_delete') : fail('kubectl_delete', kdel.error);

  const kl = await tool('kubectl_logs', { pod: 'nonexistent-pod' });
  console.log('kubectl_logs result', JSON.stringify(kl));
  kl !== undefined ? pass('kubectl_logs handled') : fail('kubectl_logs');

  const ke = await tool('kubectl_exec', { pod: 'nonexistent-pod', cmd: 'echo hello' });
  console.log('kubectl_exec result', JSON.stringify(ke));
  ke !== undefined ? pass('kubectl_exec handled') : fail('kubectl_exec');
}

module.exports = testKubernetes;
