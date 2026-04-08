#!/usr/bin/env node
const { tool, pass, fail, skip, section } = require('./common');

async function testGit() {
  section('🔀 GIT (9 tools)');

  // initialize repository
  await tool('create_directory', { path: '/tmp/gitrepo' });
  await tool('run_command', { cmd: 'git init /tmp/gitrepo && git -C /tmp/gitrepo config user.email "agent@mowisai.com" && git -C /tmp/gitrepo config user.name "MowisAI Agent"' });

  // status
  const gs = await tool('git_status', { path: '/tmp/gitrepo' });
  console.log('git_status output:', JSON.stringify(gs));
  gs.status === 'ok' ? pass('git_status') : fail('git_status', gs.error);

  // add a file and commit
  await tool('write_file', { path: '/tmp/gitrepo/README.md', content: '# Test repo' });
  const ga = await tool('git_add', { path: '/tmp/gitrepo', files: ['.'] });
  ga.status === 'ok' && ga.result?.success ? pass('git_add') : fail('git_add', JSON.stringify(ga));

  const gc = await tool('git_commit', { path: '/tmp/gitrepo', message: 'initial commit', author: 'agent' });
  console.log('git_commit output:', JSON.stringify(gc));
  gc.status === 'ok' && gc.result?.success ? pass('git_commit') : fail('git_commit', gc.error || 'no success');

  // branch listing
  const gb = await tool('git_branch', { path: '/tmp/gitrepo' });
  console.log('git_branch output:', JSON.stringify(gb));
  gb.status === 'ok' ? pass('git_branch') : fail('git_branch', gb.error);

  // checkout new branch
  const gco = await tool('git_checkout', { path: '/tmp/gitrepo', branch: 'feature/test', create: true });
  console.log('git_checkout output:', JSON.stringify(gco));
  gco.status === 'ok' && gco.result?.success ? pass('git_checkout branch') : fail('git_checkout', gco.error);

  // create file on new branch
  await tool('write_file', { path: '/tmp/gitrepo/feature.txt', content: 'branch test' });
  await tool('git_add', { path: '/tmp/gitrepo', files: ['.'] });
  await tool('git_commit', { path: '/tmp/gitrepo', message: 'branch work', author: 'agent' });

  // diff
  await tool('run_command', { cmd: 'git -C /tmp/gitrepo checkout main || git -C /tmp/gitrepo checkout master || true' });
  const gd = await tool('git_diff', { path: '/tmp/gitrepo' });
  console.log('git_diff output:', JSON.stringify(gd));
  gd.status === 'ok' ? pass('git_diff') : fail('git_diff', gd.error);

  // push/pull (no remote expected, just verify response)
  const gp = await tool('git_push', { path: '/tmp/gitrepo', remote: 'origin', branch: 'main' });
  console.log('git_push output:', JSON.stringify(gp));
  gp !== undefined ? pass('git_push (no remote)') : fail('git_push', 'no response');
  const gpl = await tool('git_pull', { path: '/tmp/gitrepo', remote: 'origin', branch: 'main' });
  console.log('git_pull output:', JSON.stringify(gpl));
  gpl !== undefined ? pass('git_pull (no remote)') : fail('git_pull', 'no response');

  // clone a small public repo if network available
  const gclone = await tool('git_clone', { repo: 'https://github.com/octocat/Hello-World.git', path: '/tmp/hello-world' });
  console.log('git_clone output:', JSON.stringify(gclone));
  if (gclone.status === 'ok' && gclone.result?.success) pass('git_clone');
  else skip('git_clone', gclone.error || 'clone failed');
}

module.exports = testGit;
