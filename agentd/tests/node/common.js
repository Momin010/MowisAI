#!/usr/bin/env node
/**
 * common utilities for the agentd functional test suite
 */

const net = require('net');
let SANDBOX_ID, CONTAINER_ID;

const results = { pass: 0, fail: 0, skip: 0, errors: [] };

function send(msg) {
  return new Promise((res, rej) => {
    const c = net.createConnection('/tmp/agentd.sock', () => c.write(JSON.stringify(msg) + '\n'));
    let d = '';
    c.on('data', x => {
      d += x;
      if (d.includes('\n')) {
        c.end();
        try {
          res(JSON.parse(d.trim()));
        } catch (e) {
          rej(e);
        }
      }
    });
    c.on('error', rej);
    setTimeout(() => {
      c.destroy();
      rej(new Error('timeout'));
    }, 30000);
  });
}

async function tool(name, input) {
  return send({ request_type: 'invoke_tool', sandbox: SANDBOX_ID, container: CONTAINER_ID, name, input });
}

function pass(name) {
  results.pass++;
  console.log(`  ✅ ${name}`);
}
function fail(name, reason) {
  results.fail++;
  results.errors.push(`${name}: ${reason}`);
  console.log(`  ❌ ${name}: ${reason}`);
}
function skip(name, reason) {
  results.skip++;
  console.log(`  ⚠️  ${name}: ${reason}`);
}
function section(t) {
  console.log(`\n${'─'.repeat(55)}\n  ${t}\n${'─'.repeat(55)}`);
}

async function setup() {
  console.log('🔧 Creating sandbox + container...');
  // first attempt to create sandbox using a base image; if the environment
  // prohibits overlay mounts (common in unprivileged CI containers) we'll
  // retry without specifying an image so it falls back to an empty tmpfs.
  let sb = await send({ request_type: 'create_sandbox', ram: 536870912, cpu: 500000, image: 'alpine' });
  if (sb.status !== 'ok') {
    console.warn('⚠️  sandbox creation with image failed, retrying without image');
    sb = await send({ request_type: 'create_sandbox', ram: 536870912, cpu: 500000 });
    if (sb.status !== 'ok') {
      console.error('❌ Sandbox failed:', sb.error);
      process.exit(1);
    }
  }
  SANDBOX_ID = sb.result.sandbox;

  const ct = await send({ request_type: 'create_container', sandbox: SANDBOX_ID });
  if (ct.status !== 'ok') { console.error('❌ Container failed:', ct.error); process.exit(1); }
  CONTAINER_ID = ct.result.container;
  console.log(`✅ Sandbox: ${SANDBOX_ID}`);
  console.log(`✅ Container: ${CONTAINER_ID}\n`);
}

function summary() {
  console.log(`\n${'═'.repeat(55)}`);
  console.log('  FINAL RESULTS');
  console.log('═'.repeat(55));
  console.log(`  ✅ Passed:  ${results.pass}`);
  console.log(`  ❌ Failed:  ${results.fail}`);
  console.log(`  ⚠️  Skipped: ${results.skip} (needs external deps?)`);
  if (results.errors.length > 0) {
    console.log('\n  🔴 FAILURES:');
    results.errors.forEach(e => console.log(`    - ${e}`));
  }
  console.log('');
}

module.exports = { send, tool, pass, fail, skip, section, setup, summary };
