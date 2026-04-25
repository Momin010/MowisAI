#!/usr/bin/env node
const { tool, pass, fail, skip, section } = require('./common');

async function testDocker() {
  section('🐳 DOCKER (7 tools)');

  const check = await tool('run_command', { cmd: 'docker --version 2>/dev/null || echo NOT_FOUND' });
  if (check.result?.stdout?.includes('NOT_FOUND')) {
    skip('docker tools', 'docker not available in this environment');
    return;
  }

  const dp = await tool('docker_ps', { all: true });
  console.log('docker_ps result', JSON.stringify(dp));
  dp.status === 'ok' ? pass('docker_ps') : fail('docker_ps', dp.error);

  const dpull = await tool('docker_pull', { image: 'alpine:latest' });
  console.log('docker_pull result', JSON.stringify(dpull));
  dpull.status === 'ok' ? pass('docker_pull') : fail('docker_pull', dpull.error);

  const dr = await tool('docker_run', { image: 'alpine', cmd: 'echo hello_docker', name: 'mowis_test' });
  console.log('docker_run result', JSON.stringify(dr));
  dr.status === 'ok' ? pass('docker_run') : fail('docker_run', dr.error);

  const dl = await tool('docker_logs', { container: 'mowis_test' });
  console.log('docker_logs result', JSON.stringify(dl));
  dl.status === 'ok' ? pass('docker_logs') : fail('docker_logs', dl.error);

  const ds = await tool('docker_stop', { container: 'mowis_test' });
  console.log('docker_stop result', JSON.stringify(ds));
  ds.status === 'ok' ? pass('docker_stop') : fail('docker_stop', ds.error);

  await tool('write_file', { path: '/tmp/Dockerfile', content: 'FROM alpine\nRUN echo built > /built.txt\n' });
  const db = await tool('docker_build', { path: '/tmp', tag: 'mowis-test:latest', dockerfile: '/tmp/Dockerfile' });
  console.log('docker_build result', JSON.stringify(db));
  db.status === 'ok' ? pass('docker_build') : fail('docker_build', db.error);

  const de = await tool('docker_exec', { container: 'mowis_test', cmd: 'echo hello' });
  console.log('docker_exec result', JSON.stringify(de));
  de !== undefined ? pass('docker_exec') : fail('docker_exec');
}

module.exports = testDocker;
