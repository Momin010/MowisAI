#!/usr/bin/env node
const { tool, pass, fail, skip, section } = require('./common');

async function testData() {
  section('🗃️  DATA / JSON (5 tools)');

  const jp = await tool('json_parse', { data: '{"name":"mowisai","version":1,"active":true}' });
  console.log('json_parse result', JSON.stringify(jp));
  jp.status === 'ok' && jp.result?.parsed?.name === 'mowisai' ? pass('json_parse') : fail('json_parse', jp.error);

  const js = await tool('json_stringify', { data: { name: 'mowisai', version: 1 } });
  console.log('json_stringify result', JSON.stringify(js));
  js.status === 'ok' && js.result?.string ? pass('json_stringify') : fail('json_stringify', js.error);

  const jq = await tool('json_query', { data: '{"users":[{"name":"alice"},{"name":"bob"}]}', path: '$.users[0].name' });
  console.log('json_query result', JSON.stringify(jq));
  jq.status === 'ok' ? pass('json_query') : fail('json_query', jq.error);

  const cw = await tool('csv_write', { path: '/tmp/test.csv', rows: [['name', 'age'], ['alice', '30'], ['bob', '25']] });
  console.log('csv_write result', JSON.stringify(cw));
  cw.status === 'ok' ? pass('csv_write') : fail('csv_write', cw.error);

  const cr = await tool('csv_read', { path: '/tmp/test.csv' });
  console.log('csv_read result', JSON.stringify(cr));
  if (cr.status === 'ok') {
    cr.result && JSON.stringify(cr.result).includes('alice') ? pass('csv_read') : fail('csv_read content');
  } else fail('csv_read', cr.error);
}

module.exports = testData;
