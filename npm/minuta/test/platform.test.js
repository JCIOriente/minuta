'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { TARGETS, resolveTarget } = require('../lib/platform');
const { locate } = require('../lib/launcher');

const CASES = [
  ['darwin', 'arm64', '@jcioriente/minuta-darwin-arm64', 'aarch64-apple-darwin'],
  ['darwin', 'x64', '@jcioriente/minuta-darwin-x64', 'x86_64-apple-darwin'],
  ['linux', 'x64', '@jcioriente/minuta-linux-x64', 'x86_64-unknown-linux-musl'],
  ['linux', 'arm64', '@jcioriente/minuta-linux-arm64', 'aarch64-unknown-linux-musl'],
];

for (const [platform, arch, pkg, rustTarget] of CASES) {
  test(`platform_maps_${platform}_${arch}`, () => {
    assert.deepEqual(resolveTarget(platform, arch), { pkg, rustTarget });
  });
}

test('platform_table_has_exactly_the_supported_pairs', () => {
  assert.equal(TARGETS.length, CASES.length);
  assert.deepEqual(
    TARGETS.map((t) => [t.platform, t.arch, t.pkg, t.rustTarget]),
    CASES,
  );
});

test('platform_rejects_unknown_pair', () => {
  assert.equal(resolveTarget('linux', 'riscv64'), null);
  assert.equal(resolveTarget('freebsd', 'x64'), null);
  assert.equal(resolveTarget('win32', 'x64'), null);
});

test('win32_message_points_to_wsl2', () => {
  const r = locate({ platform: 'win32', arch: 'x64', resolve: () => assert.fail('no lookup on win32') });
  assert.deepEqual(r, {
    ok: false,
    message:
      'minuta needs WSL2 on Windows. Install it with `wsl --install`, then run ' +
      '`npm i -g @jcioriente/minuta` inside WSL2.\n' +
      'Guide: https://github.com/JCIOriente/minuta/blob/main/docs/install.md',
  });
});

test('unlisted_platform_message_names_platform_and_cargo', () => {
  const r = locate({ platform: 'linux', arch: 'riscv64', resolve: () => assert.fail('no lookup') });
  assert.deepEqual(r, {
    ok: false,
    message:
      'minuta has no prebuilt binary for linux-riscv64.\n' +
      'Install it from source instead: cargo install --locked minuta',
  });
});

test('missing_package_message_names_package', () => {
  const resolve = () => {
    throw Object.assign(new Error('nope'), { code: 'MODULE_NOT_FOUND' });
  };
  const r = locate({ platform: 'darwin', arch: 'arm64', resolve });
  assert.deepEqual(r, {
    ok: false,
    message:
      'The package @jcioriente/minuta-darwin-arm64 is not installed, so there is no minuta binary for this machine.\n' +
      'Reinstall without --omit=optional, on the same OS this node_modules was installed on: ' +
      'npm i -g @jcioriente/minuta',
  });
});

test('locate_resolves_binary_path_inside_platform_package', () => {
  const seen = [];
  const resolve = (id) => {
    seen.push(id);
    return '/pkg/bin/minuta';
  };
  const r = locate({ platform: 'linux', arch: 'x64', resolve });
  assert.deepEqual(seen, ['@jcioriente/minuta-linux-x64/bin/minuta']);
  assert.deepEqual(r, { ok: true, binary: '/pkg/bin/minuta' });
});

test('locate_rethrows_unexpected_resolve_errors', () => {
  const resolve = () => {
    throw new Error('EACCES');
  };
  assert.throws(() => locate({ platform: 'linux', arch: 'x64', resolve }), /EACCES/);
});
