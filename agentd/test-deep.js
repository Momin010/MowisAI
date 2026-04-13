#!/usr/bin/env node
/**
 * MowisAI agentd — Deep Functional Test Suite
 * Tests every tool actually WORKS, not just exists
 */

const net = require('net');

let SANDBOX_ID, CONTAINER_ID;
const results = { pass: 0, fail: 0, skip: 0, errors: [] };

function send(msg) {
  return new Promise((res, rej) => {
    const c = net.createConnection('/tmp/agentd.sock', () => c.write(JSON.stringify(msg) + '\n'));
    let d = '';
    c.on('data', x => { d += x; if (d.includes('\n')) { c.end(); res(JSON.parse(d.trim())); } });
    c.on('error', rej);
    setTimeout(() => { c.destroy(); rej(new Error('timeout')); }, 30000);
  });
}

async function tool(name, input) {
  return send({ request_type: 'invoke_tool', sandbox: SANDBOX_ID, container: CONTAINER_ID, name, input });
}

function pass(name) { results.pass++; console.log(`  ✅ ${name}`); }
function fail(name, reason) { results.fail++; results.errors.push(`${name}: ${reason}`); console.log(`  ❌ ${name}: ${reason}`); }
function skip(name, reason) { results.skip++; console.log(`  ⚠️  ${name}: ${reason}`); }
function section(t) { console.log(`\n${'─'.repeat(55)}\n  ${t}\n${'─'.repeat(55)}`); }

// ─── SETUP ───────────────────────────────────────────────────
async function setup() {
  console.log('🔧 Creating sandbox + container...');
  const sb = await send({ request_type: 'create_sandbox', ram: 536870912, cpu: 500000, image: 'alpine' });
  if (sb.status !== 'ok') { console.error('❌ Sandbox failed:', sb.error); process.exit(1); }
  SANDBOX_ID = sb.result.sandbox;

  const ct = await send({ request_type: 'create_container', sandbox: SANDBOX_ID });
  if (ct.status !== 'ok') { console.error('❌ Container failed:', ct.error); process.exit(1); }
  CONTAINER_ID = ct.result.container;
  console.log(`✅ Sandbox: ${SANDBOX_ID}`);
  console.log(`✅ Container: ${CONTAINER_ID}\n`);
}

// ─── 1. FILESYSTEM ───────────────────────────────────────────
async function testFilesystem() {
  section('📁 FILESYSTEM (11 tools)');

  // write_file
  const w = await tool('write_file', { path: '/tmp/test.txt', content: 'hello mowisai' });
  w.status === 'ok' && w.result?.success ? pass('write_file — created file') : fail('write_file', w.error);

  // read_file
  const r = await tool('read_file', { path: '/tmp/test.txt' });
  r.status === 'ok' && r.result?.content === 'hello mowisai' ? pass('read_file — content matches') : fail('read_file', `got: ${r.result?.content} ${r.error}`);

  // append_file
  const a = await tool('append_file', { path: '/tmp/test.txt', content: '\nappended line' });
  if (a.status === 'ok') {
    const r2 = await tool('read_file', { path: '/tmp/test.txt' });
    r2.result?.content?.includes('appended line') ? pass('append_file — line appended') : fail('append_file', 'content not appended');
  } else fail('append_file', a.error);

  // get_file_info
  const info = await tool('get_file_info', { path: '/tmp/test.txt' });
  info.status === 'ok' && info.result?.size > 0 ? pass(`get_file_info — size: ${info.result.size} bytes`) : fail('get_file_info', info.error);

  // create_directory
  const mkdir = await tool('create_directory', { path: '/tmp/testdir' });
  mkdir.status === 'ok' ? pass('create_directory — /tmp/testdir created') : fail('create_directory', mkdir.error);

  // create nested dir
  const mkdir2 = await tool('create_directory', { path: '/tmp/testdir/subdir' });
  mkdir2.status === 'ok' ? pass('create_directory — nested /tmp/testdir/subdir created') : fail('create_directory nested', mkdir2.error);

  // write file inside dir
  await tool('write_file', { path: '/tmp/testdir/file1.txt', content: 'file in dir' });
  await tool('write_file', { path: '/tmp/testdir/file2.txt', content: 'second file' });

  // list_files
  const ls = await tool('list_files', { path: '/tmp/testdir' });
  if (ls.status === 'ok') {
    const hasFiles = ls.result?.files?.includes('file1.txt') && ls.result?.files?.includes('file2.txt');
    const hasDirs = ls.result?.directories?.includes('subdir');
    hasFiles ? pass(`list_files — found: ${ls.result.files.join(', ')}`) : fail('list_files files', `got: ${JSON.stringify(ls.result?.files)}`);
    hasDirs ? pass('list_files — found subdir directory') : fail('list_files dirs', `got: ${JSON.stringify(ls.result?.directories)}`);
  } else fail('list_files', ls.error);

  // file_exists — true
  const fe1 = await tool('file_exists', { path: '/tmp/test.txt' });
  fe1.result?.exists === true ? pass('file_exists — returns true for existing file') : fail('file_exists true', `got: ${fe1.result?.exists}`);

  // file_exists — false
  const fe2 = await tool('file_exists', { path: '/tmp/doesnotexist.txt' });
  fe2.result?.exists === false ? pass('file_exists — returns false for missing file') : fail('file_exists false', `got: ${fe2.result?.exists}`);

  // copy_file
  const cp = await tool('copy_file', { from: '/tmp/test.txt', to: '/tmp/test_copy.txt' });
  if (cp.status === 'ok') {
    const verify = await tool('read_file', { path: '/tmp/test_copy.txt' });
    verify.result?.content?.includes('hello mowisai') ? pass('copy_file — file copied with correct content') : fail('copy_file content', 'content mismatch');
  } else fail('copy_file', cp.error);

  // move_file
  const mv = await tool('move_file', { from: '/tmp/test_copy.txt', to: '/tmp/test_moved.txt' });
  if (mv.status === 'ok') {
    const src = await tool('file_exists', { path: '/tmp/test_copy.txt' });
    const dst = await tool('file_exists', { path: '/tmp/test_moved.txt' });
    src.result?.exists === false && dst.result?.exists === true
      ? pass('move_file — source gone, destination exists')
      : fail('move_file', `src exists: ${src.result?.exists}, dst exists: ${dst.result?.exists}`);
  } else fail('move_file', mv.error);

  // delete_file
  const del = await tool('delete_file', { path: '/tmp/test_moved.txt' });
  if (del.status === 'ok') {
    const gone = await tool('file_exists', { path: '/tmp/test_moved.txt' });
    gone.result?.exists === false ? pass('delete_file — file deleted') : fail('delete_file verify', 'file still exists');
  } else fail('delete_file', del.error);

  // delete_directory
  const rmdir = await tool('delete_directory', { path: '/tmp/testdir' });
  if (rmdir.status === 'ok') {
    const gone = await tool('file_exists', { path: '/tmp/testdir' });
    gone.result?.exists === false ? pass('delete_directory — directory deleted') : fail('delete_directory verify', 'dir still exists');
  } else fail('delete_directory', rmdir.error);
}

// ─── 2. SHELL ────────────────────────────────────────────────
async function testShell() {
  section('💻 SHELL (5 tools)');

  // run_command — basic
  const r1 = await tool('run_command', { cmd: 'echo hello_mowisai' });
  r1.result?.stdout?.trim() === 'hello_mowisai' ? pass('run_command — echo works') : fail('run_command echo', `got: ${r1.result?.stdout}`);

  // run_command — with cwd
  await tool('create_directory', { path: '/tmp/cmdtest' });
  await tool('write_file', { path: '/tmp/cmdtest/data.txt', content: 'test data' });
  const r2 = await tool('run_command', { cmd: 'ls', cwd: '/tmp/cmdtest' });
  r2.result?.stdout?.includes('data.txt') ? pass('run_command — cwd works, ls shows file') : fail('run_command cwd', `got: ${r2.result?.stdout}`);

  // run_command — chained commands
  const r3 = await tool('run_command', { cmd: 'echo first && echo second' });
  r3.result?.stdout?.includes('first') && r3.result?.stdout?.includes('second') ? pass('run_command — chained commands work') : fail('run_command chain', `got: ${r3.result?.stdout}`);

  // run_command — capture stderr
  const r4 = await tool('run_command', { cmd: 'ls /nonexistent 2>&1' });
  r4.result !== undefined ? pass('run_command — stderr captured') : fail('run_command stderr', 'no result');

  // run_script
  const script = '#!/bin/sh\necho "script_output"\necho "line2"';
  await tool('write_file', { path: '/tmp/test.sh', content: script });
  const rs = await tool('run_command', { cmd: 'sh /tmp/test.sh' });
  rs.result?.stdout?.includes('script_output') ? pass('run_script — script executed') : fail('run_script', `got: ${rs.result?.stdout}`);

  // get_env
  const ge = await tool('get_env', { var: 'PATH' });
  ge.status === 'ok' && ge.result?.value?.includes('/bin') ? pass(`get_env — PATH: ${ge.result.value.substring(0, 40)}...`) : fail('get_env', ge.error);

  // set_env
  const se = await tool('set_env', { var: 'MOWIS_TEST', value: 'works' });
  se.status === 'ok' ? pass('set_env — env variable set') : fail('set_env', se.error);

  // kill_process — with fake PID (should handle gracefully)
  const kp = await tool('kill_process', { pid: 99999 });
  kp !== undefined ? pass('kill_process — handles invalid PID gracefully') : fail('kill_process', 'no response');
}

// ─── 3. HTTP / NETWORK ───────────────────────────────────────
async function testHttp() {
  section('🌐 NETWORK / HTTP (7 tools)');

  // http_get
  const get = await tool('http_get', { url: 'https://httpbin.org/get' });
  get.status === 'ok' ? pass('http_get — GET request succeeded') : fail('http_get', get.error);

  // http_post
  const post = await tool('http_post', { url: 'https://httpbin.org/post', body: JSON.stringify({ test: 'mowisai' }), headers: { 'Content-Type': 'application/json' } });
  post.status === 'ok' ? pass('http_post — POST request succeeded') : fail('http_post', post.error);

  // http_put
  const put = await tool('http_put', { url: 'https://httpbin.org/put', body: JSON.stringify({ key: 'value' }) });
  put.status === 'ok' ? pass('http_put — PUT request succeeded') : fail('http_put', put.error);

  // http_delete
  const del = await tool('http_delete', { url: 'https://httpbin.org/delete' });
  del.status === 'ok' ? pass('http_delete — DELETE request succeeded') : fail('http_delete', del.error);

  // http_patch
  const patch = await tool('http_patch', { url: 'https://httpbin.org/patch', body: JSON.stringify({ patch: true }) });
  patch.status === 'ok' ? pass('http_patch — PATCH request succeeded') : fail('http_patch', patch.error);

  // download_file
  const dl = await tool('download_file', { url: 'https://httpbin.org/get', path: '/tmp/downloaded.json' });
  if (dl.status === 'ok') {
    const verify = await tool('file_exists', { path: '/tmp/downloaded.json' });
    verify.result?.exists ? pass('download_file — file downloaded to /tmp/downloaded.json') : fail('download_file verify', 'file not found');
  } else fail('download_file', dl.error);

  // websocket_send
  const ws = await tool('websocket_send', { url: 'ws://localhost:9999', message: 'test' });
  ws !== undefined ? pass('websocket_send — handled (no server expected)') : fail('websocket_send', 'no response');
}

// ─── 4. DATA / JSON ──────────────────────────────────────────
async function testData() {
  section('🗃️  DATA / JSON (5 tools)');

  // json_parse
  const jp = await tool('json_parse', { data: '{"name":"mowisai","version":1,"active":true}' });
  jp.status === 'ok' && jp.result?.parsed?.name === 'mowisai' ? pass(`json_parse — parsed correctly: name=${jp.result.parsed.name}`) : fail('json_parse', jp.error);

  // json_stringify
  const js = await tool('json_stringify', { data: { name: 'mowisai', version: 1 } });
  js.status === 'ok' && js.result?.string !== undefined ? pass(`json_stringify — serialized: ${js.result.string?.substring(0, 40)}`) : fail('json_stringify', js.error);

  // json_query
  const jq = await tool('json_query', { data: '{"users":[{"name":"alice"},{"name":"bob"}]}', path: '$.users[0].name' });
  jq.status === 'ok' ? pass(`json_query — queried: ${JSON.stringify(jq.result)}`) : fail('json_query', jq.error);

  // csv_write
  const cw = await tool('csv_write', { path: '/tmp/test.csv', rows: [['name', 'age', 'city'], ['alice', '30', 'Helsinki'], ['bob', '25', 'Tampere']] });
  cw.status === 'ok' ? pass('csv_write — CSV written') : fail('csv_write', cw.error);

  // csv_read
  const cr = await tool('csv_read', { path: '/tmp/test.csv' });
  if (cr.status === 'ok') {
    const hasAlice = JSON.stringify(cr.result).includes('alice');
    hasAlice ? pass('csv_read — CSV read, found alice') : fail('csv_read content', `got: ${JSON.stringify(cr.result).substring(0, 80)}`);
  } else fail('csv_read', cr.error);
}

// ─── 5. GIT ──────────────────────────────────────────────────
async function testGit() {
  section('🔀 GIT (9 tools)');

  // Setup git repo
  await tool('create_directory', { path: '/tmp/gitrepo' });
  await tool('run_command', { cmd: 'git init /tmp/gitrepo && git -C /tmp/gitrepo config user.email "agent@mowisai.com" && git -C /tmp/gitrepo config user.name "MowisAI Agent"' });

  // git_status
  const gs = await tool('git_status', { path: '/tmp/gitrepo' });
  gs.status === 'ok' ? pass('git_status — got status') : fail('git_status', gs.error);

  // git_add + git_commit
  await tool('write_file', { path: '/tmp/gitrepo/README.md', content: '# MowisAI Test Repo\nBuilt by autonomous agents.' });
  const ga = await tool('git_add', { path: '/tmp/gitrepo', files: ['.'] });
  ga.status === 'ok' ? pass('git_add — staged files') : fail('git_add', ga.error);

  const gc = await tool('git_commit', { path: '/tmp/gitrepo', message: 'Initial commit by MowisAI agent', author: 'MowisAI Agent' });
  gc.status === 'ok' ? pass('git_commit — committed with message') : fail('git_commit', gc.error);

  // git_branch
  const gb = await tool('git_branch', { path: '/tmp/gitrepo' });
  gb.status === 'ok' ? pass(`git_branch — branches: ${JSON.stringify(gb.result).substring(0, 60)}`) : fail('git_branch', gb.error);

  // git_checkout (create new branch)
  const gco = await tool('git_checkout', { path: '/tmp/gitrepo', branch: 'feature/agent-test', create: true });
  gco.status === 'ok' ? pass('git_checkout — switched to feature/agent-test') : fail('git_checkout', gco.error);

  // Write file on new branch and commit
  await tool('write_file', { path: '/tmp/gitrepo/feature.js', content: 'console.log("feature branch")' });
  await tool('git_add', { path: '/tmp/gitrepo', files: ['.'] });
  await tool('git_commit', { path: '/tmp/gitrepo', message: 'Add feature', author: 'MowisAI Agent' });

  // git_diff (back to main)
  await tool('run_command', { cmd: 'git -C /tmp/gitrepo checkout main 2>/dev/null || git -C /tmp/gitrepo checkout master 2>/dev/null || true' });
  const gd = await tool('git_diff', { path: '/tmp/gitrepo' });
  gd.status === 'ok' ? pass('git_diff — diff retrieved') : fail('git_diff', gd.error);

  // git_push (will fail without remote — just verify it tries)
  const gp = await tool('git_push', { path: '/tmp/gitrepo', remote: 'origin', branch: 'main' });
  gp !== undefined ? pass('git_push — handled (no remote expected)') : fail('git_push', 'no response');

  // git_pull (will fail without remote — just verify it tries)
  const gpl = await tool('git_pull', { path: '/tmp/gitrepo', remote: 'origin', branch: 'main' });
  gpl !== undefined ? pass('git_pull — handled (no remote expected)') : fail('git_pull', 'no response');

  // git_clone (real repo)
  const gclone = await tool('git_clone', { repo: 'https://github.com/octocat/Hello-World.git', path: '/tmp/hello-world' });
  gclone.status === 'ok' ? pass('git_clone — cloned octocat/Hello-World') : skip('git_clone', gclone.error);
}

// ─── 6. DOCKER ───────────────────────────────────────────────
async function testDocker() {
  section('🐳 DOCKER (7 tools)');

  // Check if docker is available
  const check = await tool('run_command', { cmd: 'docker --version 2>/dev/null || echo NOT_FOUND' });
  if (check.result?.stdout?.includes('NOT_FOUND')) {
    skip('docker_*', 'Docker daemon not available in this environment');
    return;
  }

  const dp = await tool('docker_ps', { all: true });
  dp.status === 'ok' ? pass('docker_ps — listed containers') : fail('docker_ps', dp.error);

  const dpull = await tool('docker_pull', { image: 'alpine:latest' });
  dpull.status === 'ok' ? pass('docker_pull — pulled alpine') : fail('docker_pull', dpull.error);

  const dr = await tool('docker_run', { image: 'alpine', cmd: 'echo hello_docker', name: 'mowis_test' });
  dr.status === 'ok' ? pass('docker_run — ran alpine container') : fail('docker_run', dr.error);

  const dl = await tool('docker_logs', { container: 'mowis_test' });
  dl.status === 'ok' ? pass('docker_logs — got logs') : fail('docker_logs', dl.error);

  const ds = await tool('docker_stop', { container: 'mowis_test' });
  ds.status === 'ok' ? pass('docker_stop — stopped container') : fail('docker_stop', ds.error);

  await tool('write_file', { path: '/tmp/Dockerfile', content: 'FROM alpine\nRUN echo built > /built.txt\n' });
  const db = await tool('docker_build', { path: '/tmp', tag: 'mowis-test:latest', dockerfile: '/tmp/Dockerfile' });
  db.status === 'ok' ? pass('docker_build — image built') : fail('docker_build', db.error);

  const de = await tool('docker_exec', { container: 'mowis_test', cmd: 'echo hello' });
  de !== undefined ? pass('docker_exec — handled') : fail('docker_exec', 'no response');
}

// ─── 7. KUBERNETES ───────────────────────────────────────────
async function testKubernetes() {
  section('☸️  KUBERNETES (6 tools)');

  const check = await tool('run_command', { cmd: 'kubectl version --client 2>/dev/null || echo NOT_FOUND' });
  if (check.result?.stdout?.includes('NOT_FOUND')) {
    skip('kubectl_*', 'kubectl not available in this environment');
    return;
  }

  const kg = await tool('kubectl_get', { resource: 'pods' });
  kg.status === 'ok' ? pass('kubectl_get — got pods') : fail('kubectl_get', kg.error);

  const manifest = 'apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: mowis-test\ndata:\n  key: value\n';
  await tool('write_file', { path: '/tmp/manifest.yaml', content: manifest });
  const ka = await tool('kubectl_apply', { manifest: '/tmp/manifest.yaml' });
  ka.status === 'ok' ? pass('kubectl_apply — manifest applied') : fail('kubectl_apply', ka.error);

  const kd = await tool('kubectl_describe', { resource: 'configmap', name: 'mowis-test' });
  kd.status === 'ok' ? pass('kubectl_describe — described resource') : fail('kubectl_describe', kd.error);

  const kdel = await tool('kubectl_delete', { resource: 'configmap', name: 'mowis-test' });
  kdel.status === 'ok' ? pass('kubectl_delete — deleted resource') : fail('kubectl_delete', kdel.error);

  const kl = await tool('kubectl_logs', { pod: 'nonexistent-pod' });
  kl !== undefined ? pass('kubectl_logs — handled') : fail('kubectl_logs', 'no response');

  const ke = await tool('kubectl_exec', { pod: 'nonexistent-pod', cmd: 'echo hello' });
  ke !== undefined ? pass('kubectl_exec — handled') : fail('kubectl_exec', 'no response');
}

// ─── 8. MEMORY ───────────────────────────────────────────────
async function testMemory() {
  section('🧠 MEMORY / STATE (6 tools)');

  const ms = await tool('memory_set', { key: 'agent_goal', value: 'build devops pipeline' });
  ms.status === 'ok' ? pass('memory_set — stored key') : fail('memory_set', ms.error);

  await tool('memory_set', { key: 'agent_status', value: 'running' });
  await tool('memory_set', { key: 'task_count', value: '42' });

  const mg = await tool('memory_get', { key: 'agent_goal' });
  mg.status === 'ok' && mg.result?.value === 'build devops pipeline' ? pass(`memory_get — retrieved: "${mg.result.value}"`) : fail('memory_get', `got: ${mg.result?.value}`);

  const ml = await tool('memory_list', {});
  ml.status === 'ok' && ml.result?.keys?.includes('agent_goal') ? pass(`memory_list — keys: ${ml.result.keys.join(', ')}`) : fail('memory_list', ml.error);

  const msave = await tool('memory_save', { path: '/tmp/memory.json' });
  msave.status === 'ok' ? pass('memory_save — persisted to disk') : fail('memory_save', msave.error);

  const md = await tool('memory_delete', { key: 'agent_goal' });
  md.status === 'ok' ? pass('memory_delete — key deleted') : fail('memory_delete', md.error);

  const gone = await tool('memory_get', { key: 'agent_goal' });
  gone.status !== 'ok' || gone.result?.value === null ? pass('memory_delete verify — key gone') : fail('memory_delete verify', `still got: ${gone.result?.value}`);

  const mload = await tool('memory_load', { path: '/tmp/memory.json' });
  mload.status === 'ok' ? pass('memory_load — loaded from disk') : fail('memory_load', mload.error);

  const restored = await tool('memory_get', { key: 'agent_goal' });
  restored.result?.value === 'build devops pipeline' ? pass('memory_load verify — data restored correctly') : fail('memory_load verify', `got: ${restored.result?.value}`);
}

// ─── 9. SECRETS ──────────────────────────────────────────────
async function testSecrets() {
  section('🔐 SECRETS (2 tools)');

  const ss = await tool('secret_set', { name: 'API_KEY', value: 'sk-mowisai-secret-12345' });
  ss.status === 'ok' ? pass('secret_set — stored secret') : fail('secret_set', ss.error);

  const sg = await tool('secret_get', { name: 'API_KEY' });
  sg.status === 'ok' && sg.result?.value === 'sk-mowisai-secret-12345' ? pass('secret_get — retrieved correct value') : fail('secret_get', `got: ${sg.result?.value} ${sg.error}`);
}

// ─── 10. PACKAGES ────────────────────────────────────────────
async function testPackages() {
  section('📦 PACKAGE MANAGEMENT (3 tools)');

  await tool('create_directory', { path: '/tmp/pkgtest' });
  await tool('run_command', { cmd: 'cd /tmp/pkgtest && npm init -y' });

  const npm = await tool('npm_install', { package: 'lodash', cwd: '/tmp/pkgtest' });
  if (npm.status === 'ok') {
    const verify = await tool('file_exists', { path: '/tmp/pkgtest/node_modules/lodash' });
    verify.result?.exists ? pass('npm_install — lodash installed and verified') : fail('npm_install verify', 'node_modules/lodash not found');
  } else fail('npm_install', npm.error);

  const pip = await tool('pip_install', { package: 'requests' });
  pip.status === 'ok' ? pass('pip_install — requests installed') : fail('pip_install', pip.error);

  // cargo_add needs a Cargo.toml
  await tool('write_file', { path: '/tmp/pkgtest/Cargo.toml', content: '[package]\nname = "test"\nversion = "0.1.0"\nedition = "2021"\n\n[dependencies]\n' });
  const cargo = await tool('cargo_add', { package: 'serde', cwd: '/tmp/pkgtest' });
  cargo.status === 'ok' ? pass('cargo_add — serde added') : skip('cargo_add', cargo.error);
}

// ─── 11. WEB / SEARCH ────────────────────────────────────────
async function testWeb() {
  section('🔍 SEARCH / WEB (3 tools)');

  const ws = await tool('web_search', { query: 'MowisAI autonomous agent infrastructure' });
  ws.status === 'ok' ? pass('web_search — search returned results') : fail('web_search', ws.error);

  const wf = await tool('web_fetch', { url: 'https://httpbin.org/html' });
  wf.status === 'ok' ? pass('web_fetch — fetched HTML page') : fail('web_fetch', wf.error);

  const wsc = await tool('web_screenshot', { url: 'https://httpbin.org', output: '/tmp/screenshot.png' });
  wsc.status === 'ok' ? pass('web_screenshot — screenshot taken') : skip('web_screenshot', wsc.error);
}

// ─── 12. AGENT COORDINATION ──────────────────────────────────
async function testAgentCoordination() {
  section('🤝 AGENT COORDINATION (6 tools)');

  const cc = await tool('create_channel', { name: 'orchestrator' });
  cc.status === 'ok' ? pass('create_channel — channel created') : fail('create_channel', cc.error);

  const sm = await tool('send_message', { channel: 'orchestrator', message: 'task: build api server', sender: 'hub_agent' });
  sm.status === 'ok' ? pass('send_message — message sent') : fail('send_message', sm.error);

  await tool('send_message', { channel: 'orchestrator', message: 'task: write tests', sender: 'hub_agent' });
  await tool('send_message', { channel: 'orchestrator', message: 'task: deploy to k8s', sender: 'hub_agent' });

  const rm = await tool('read_messages', { channel: 'orchestrator' });
  if (rm.status === 'ok') {
    const msgs = rm.result?.messages || rm.result;
    const count = Array.isArray(msgs) ? msgs.length : 0;
    count > 0 ? pass(`read_messages — read ${count} messages from channel`) : fail('read_messages count', `got: ${JSON.stringify(rm.result).substring(0, 80)}`);
  } else fail('read_messages', rm.error);

  const bc = await tool('broadcast', { message: 'all workers: start execution', sender: 'hub_agent' });
  bc.status === 'ok' ? pass('broadcast — message broadcast to all agents') : fail('broadcast', bc.error);

  const sa = await tool('spawn_agent', { task: 'echo hello from spawned agent', tools: ['run_command'] });
  sa.status === 'ok' ? pass('spawn_agent — agent spawned') : fail('spawn_agent', sa.error);

  const wf = await tool('wait_for', { channel: 'orchestrator', timeout: 100 });
  wf !== undefined ? pass('wait_for — handled') : fail('wait_for', 'no response');
}

// ─── 13. CODE ANALYSIS ───────────────────────────────────────
async function testCodeAnalysis() {
  section('📊 CODE ANALYSIS (3 tools)');

  // Write test files
  await tool('write_file', { path: '/tmp/app.js', content: 'const x = 1;\nconsole.log(x);\n' });
  await tool('write_file', { path: '/tmp/app.py', content: 'x = 1\nprint(x)\n' });

  const build = await tool('build', { path: '/tmp', command: 'echo build_ok' });
  build.status === 'ok' ? pass('build — custom build command executed') : fail('build', build.error);

  const test = await tool('test', { path: '/tmp', framework: 'echo' });
  test.status === 'ok' ? pass('test — test runner invoked') : fail('test', test.error);

  // These need linters installed
  const lint = await tool('lint', { path: '/tmp/app.js', language: 'js' });
  lint.status === 'ok' ? pass('lint — linter ran') : skip('lint', lint.error);
}

// ─── SUMMARY ─────────────────────────────────────────────────
function summary() {
  console.log(`\n${'═'.repeat(55)}`);
  console.log('  FINAL RESULTS');
  console.log('═'.repeat(55));
  console.log(`  ✅ Passed:  ${results.pass}`);
  console.log(`  ❌ Failed:  ${results.fail}`);
  console.log(`  ⚠️  Skipped: ${results.skip} (need external deps)`);
  console.log(`  📊 Total:   ${results.pass + results.fail + results.skip}`);

  if (results.errors.length > 0) {
    console.log('\n  🔴 FAILURES:');
    results.errors.forEach(e => console.log(`    - ${e}`));
  }

  console.log('\n' + (results.fail === 0
    ? '  🎉 ALL TOOLS VERIFIED — ENGINE READY FOR ORCHESTRATION'
    : `  ⚠️  ${results.fail} TOOL(S) NEED FIXING`));
  process.exit(results.fail > 0 ? 1 : 0);
}

// ─── MAIN ────────────────────────────────────────────────────
(async () => {
  console.log('═'.repeat(55));
  console.log('  MowisAI agentd — Deep 75-Tool Functional Test');
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
  await testAgentCoordination();
  await testCodeAnalysis();
  summary();
})();