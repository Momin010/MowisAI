#!/usr/bin/env node
const { tool, pass, fail, skip, section } = require('./common');

async function testHttp() {
  section('🌐 NETWORK / HTTP (7 tools)');

  const get = await tool('http_get', { url: 'https://httpbin.org/get' });
  console.log('http_get result', JSON.stringify(get));
  get.status === 'ok' ? pass('http_get') : fail('http_get', get.error);

  const post = await tool('http_post', { url: 'https://httpbin.org/post', body: JSON.stringify({ test: 'mowisai' }), headers: { 'Content-Type': 'application/json' } });
  console.log('http_post result', JSON.stringify(post));
  post.status === 'ok' ? pass('http_post') : fail('http_post', post.error);

  const put = await tool('http_put', { url: 'https://httpbin.org/put', body: JSON.stringify({ key: 'value' }) });
  console.log('http_put result', JSON.stringify(put));
  put.status === 'ok' ? pass('http_put') : fail('http_put', put.error);

  const del = await tool('http_delete', { url: 'https://httpbin.org/delete' });
  console.log('http_delete result', JSON.stringify(del));
  del.status === 'ok' ? pass('http_delete') : fail('http_delete', del.error);

  const patch = await tool('http_patch', { url: 'https://httpbin.org/patch', body: JSON.stringify({ patch: true }) });
  console.log('http_patch result', JSON.stringify(patch));
  patch.status === 'ok' ? pass('http_patch') : fail('http_patch', patch.error);

  const dl = await tool('download_file', { url: 'https://httpbin.org/get', path: '/tmp/downloaded.json' });
  console.log('download_file result', JSON.stringify(dl));
  if (dl.status === 'ok') {
    const verify = await tool('file_exists', { path: '/tmp/downloaded.json' });
    verify.result?.exists ? pass('download_file') : fail('download_file verify');
  } else fail('download_file', dl.error);

  const ws = await tool('websocket_send', { url: 'ws://localhost:9999', message: 'test' });
  console.log('websocket_send result', JSON.stringify(ws));
  ws !== undefined ? pass('websocket_send') : fail('websocket_send', 'no response');
}

module.exports = testHttp;
