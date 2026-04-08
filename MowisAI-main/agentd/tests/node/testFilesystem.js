#!/usr/bin/env node
const { tool, pass, fail, skip, section } = require('./common');

async function testFilesystem() {
  section('📁 FILESYSTEM (11 tools)');

  // write_file
  const w = await tool('write_file', { path: '/tmp/test.txt', content: 'hello mowisai' });
  console.log('write_file result', JSON.stringify(w));
  w.status === 'ok' && w.result?.success ? pass('write_file') : fail('write_file', w.error);

  // read_file
  const r = await tool('read_file', { path: '/tmp/test.txt' });
  console.log('read_file result', JSON.stringify(r));
  r.status === 'ok' && r.result?.content === 'hello mowisai' ? pass('read_file') : fail('read_file', `got: ${r.result?.content}`);

  // append_file
  const a = await tool('append_file', { path: '/tmp/test.txt', content: '\nappended' });
  console.log('append_file result', JSON.stringify(a));
  if (a.status === 'ok') {
    const r2 = await tool('read_file', { path: '/tmp/test.txt' });
    r2.result?.content?.includes('appended') ? pass('append_file') : fail('append_file', 'not appended');
  } else fail('append_file', a.error);

  // get_file_info
  const info = await tool('get_file_info', { path: '/tmp/test.txt' });
  console.log('get_file_info result', JSON.stringify(info));
  info.status === 'ok' && info.result?.size > 0 ? pass('get_file_info') : fail('get_file_info', info.error);

  // create_directory
  const mkdir = await tool('create_directory', { path: '/tmp/testdir' });
  mkdir.status === 'ok' ? pass('create_directory') : fail('create_directory', mkdir.error);

  // nested directory and files
  await tool('create_directory', { path: '/tmp/testdir/subdir' });
  await tool('write_file', { path: '/tmp/testdir/file1.txt', content: 'file1' });
  await tool('write_file', { path: '/tmp/testdir/file2.txt', content: 'file2' });

  // list_files
  const ls = await tool('list_files', { path: '/tmp/testdir' });
  console.log('list_files result', JSON.stringify(ls));
  if (ls.status === 'ok') {
    const hasFiles = ls.result?.files?.includes('file1.txt') && ls.result?.files?.includes('file2.txt');
    const hasDirs = ls.result?.directories?.includes('subdir');
    hasFiles ? pass('list_files files') : fail('list_files files', JSON.stringify(ls.result));
    hasDirs ? pass('list_files dirs') : fail('list_files dirs', JSON.stringify(ls.result));
  } else fail('list_files', ls.error);

  // file_exists true/false
  const fe1 = await tool('file_exists', { path: '/tmp/test.txt' });
  console.log('file_exists(true) result', JSON.stringify(fe1));
  fe1.result?.exists === true ? pass('file_exists true') : fail('file_exists true', JSON.stringify(fe1.result));
  const fe2 = await tool('file_exists', { path: '/tmp/nope.txt' });
  console.log('file_exists(false) result', JSON.stringify(fe2));
  fe2.result?.exists === false ? pass('file_exists false') : fail('file_exists false', JSON.stringify(fe2.result));

  // copy_file
  const cp = await tool('copy_file', { from: '/tmp/test.txt', to: '/tmp/test_copy.txt' });
  console.log('copy_file result', JSON.stringify(cp));
  if (cp.status === 'ok') {
    const verify = await tool('read_file', { path: '/tmp/test_copy.txt' });
    verify.result?.content?.includes('hello mowisai') ? pass('copy_file') : fail('copy_file content');
  } else fail('copy_file', cp.error);

  // move_file
  const mv = await tool('move_file', { from: '/tmp/test_copy.txt', to: '/tmp/test_moved.txt' });
  console.log('move_file result', JSON.stringify(mv));
  if (mv.status === 'ok') {
    const src = await tool('file_exists', { path: '/tmp/test_copy.txt' });
    const dst = await tool('file_exists', { path: '/tmp/test_moved.txt' });
    src.result?.exists === false && dst.result?.exists === true ? pass('move_file') : fail('move_file', `src=${src.result?.exists} dst=${dst.result?.exists}`);
  } else fail('move_file', mv.error);

  // delete_file
  const del = await tool('delete_file', { path: '/tmp/test_moved.txt' });
  console.log('delete_file result', JSON.stringify(del));
  if (del.status === 'ok') {
    const gone = await tool('file_exists', { path: '/tmp/test_moved.txt' });
    gone.result?.exists === false ? pass('delete_file') : fail('delete_file verify');
  } else fail('delete_file', del.error);

  // delete_directory
  const rmdir = await tool('delete_directory', { path: '/tmp/testdir' });
  console.log('delete_directory result', JSON.stringify(rmdir));
  if (rmdir.status === 'ok') {
    const gone = await tool('file_exists', { path: '/tmp/testdir' });
    gone.result?.exists === false ? pass('delete_directory') : fail('delete_directory verify');
  } else fail('delete_directory', rmdir.error);
}

module.exports = testFilesystem;
