#!/usr/bin/env node
const { setup, summary } = require('./common');
const testFilesystem = require('./testFilesystem');
const testShell = require('./testShell');
const testHttp = require('./testHttp');
const testData = require('./testData');
const testGit = require('./testGit');
const testDocker = require('./testDocker');
const testKubernetes = require('./testKubernetes');
const testMemory = require('./testMemory');
const testSecrets = require('./testSecrets');
const testPackages = require('./testPackages');
const testWeb = require('./testWeb');
const testChannels = require('./testChannels');
const testDevTools = require('./testDevTools');
const testUtils = require('./testUtils');

(async () => {
  console.log('═'.repeat(55));
  console.log('  MowisAI agentd — Comprehensive Tool Test Suite');
  console.log('═'.repeat(55));
  await setup();
  await testFilesystem();
  await testShell();
  await testHttp();
  await testData();
  await testGit();
  await testDocker();
  await testKubernetes();
  await testMemory();
  await testSecrets();
  await testPackages();
  await testWeb();
  await testChannels();
  await testDevTools();
  await testUtils();
  summary();
})();