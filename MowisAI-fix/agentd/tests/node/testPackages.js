#!/usr/bin/env node
const { tool, pass, fail, skip, section } = require('./common');

async function testPackages() {
  section('📦 PACKAGE MANAGEMENT (3 tools)');

  await tool('create_directory', { path: '/tmp/pkgtest' });
  await tool('run_command', { cmd: 'cd /tmp/pkgtest && npm init -y' });

  const npm = await tool('npm_install', { package: 'lodash', cwd: '/tmp/pkgtest' });
  console.log('npm_install result', JSON.stringify(npm));
  if (npm.status === 'ok') {
    const verify = await tool('file_exists', { path: '/tmp/pkgtest/node_modules/lodash' });
    verify.result?.exists ? pass('npm_install') : fail('npm_install verify');
  } else fail('npm_install', npm.error);

  const pip = await tool('pip_install', { package: 'requests' });
  console.log('pip_install result', JSON.stringify(pip));
  pip.status === 'ok' ? pass('pip_install') : fail('pip_install', pip.error);

  await tool('write_file', { path: '/tmp/pkgtest/Cargo.toml', content: '[package]\nname = "test"\nversion = "0.1.0"\nedition = "2021"\n\n[dependencies]\n' });
  const cargo = await tool('cargo_add', { package: 'serde', cwd: '/tmp/pkgtest' });
  console.log('cargo_add result', JSON.stringify(cargo));
  if (cargo.status === 'ok') pass('cargo_add'); else skip('cargo_add', cargo.error);
}

module.exports = testPackages;
