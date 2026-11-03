'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn, spawnSync } = require('node:child_process');
const { resolveTarget } = require('../lib/platform');

const SRC = path.join(__dirname, '..');
const target = resolveTarget(process.platform, process.arch);
const skip = target ? false : `no package for ${process.platform}-${process.arch}`;

// A copy of the launcher with a fake platform package next to it, as npm lays it out.
function install(script) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'minuta-launcher-'));
  const pkgDir = path.join(root, 'node_modules', ...target.pkg.split('/'));
  fs.mkdirSync(path.join(pkgDir, 'bin'), { recursive: true });
  fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({ name: target.pkg, version: '0.0.0' }));
  const bin = path.join(pkgDir, 'bin', 'minuta');
  fs.writeFileSync(bin, script, { mode: 0o755 });
  const pkg = path.join(root, 'node_modules', '@jcioriente', 'minuta');
  fs.cpSync(SRC, pkg, { recursive: true, filter: (p) => !p.includes(`${path.sep}test`) });
  return { root, launcher: path.join(pkg, 'bin', 'minuta.js') };
}

test('launcher_passes_args_and_exit_code', { skip }, () => {
  const { launcher } = install('#!/bin/sh\necho "args:$*"\nexit 7\n');
  const r = spawnSync(process.execPath, [launcher, 'run', 'a b.mp4', '-t', 'summary'], { encoding: 'utf8' });
  assert.equal(r.status, 7);
  assert.equal(r.stdout, 'args:run a b.mp4 -t summary\n');
});

test('launcher_passes_stdout_and_stderr', { skip }, () => {
  const { launcher } = install('#!/bin/sh\necho out-line\necho err-line >&2\n');
  const r = spawnSync(process.execPath, [launcher], { encoding: 'utf8' });
  assert.equal(r.status, 0);
  assert.equal(r.stdout, 'out-line\n');
  assert.equal(r.stderr, 'err-line\n');
});

test('launcher_passes_stdin', { skip }, () => {
  const { launcher } = install('#!/bin/sh\ncat\n');
  const r = spawnSync(process.execPath, [launcher], { encoding: 'utf8', input: 'from stdin' });
  assert.equal(r.stdout, 'from stdin');
});

test('launcher_reports_missing_package_and_exits_1', { skip }, () => {
  const { root, launcher } = install('#!/bin/sh\n');
  fs.rmSync(path.join(root, 'node_modules', ...target.pkg.split('/')), { recursive: true });
  const r = spawnSync(process.execPath, [launcher], { encoding: 'utf8' });
  assert.equal(r.status, 1);
  assert.equal(r.stdout, '');
  assert.match(r.stderr, new RegExp(`^The package ${target.pkg} is not installed`));
});

for (const sig of ['SIGTERM', 'SIGINT', 'SIGHUP']) {
  test(`launcher_forwards_${sig.toLowerCase()}_to_child`, { skip, timeout: 30000 }, async () => {
    // The fake binary reports readiness, then waits; it exits 42 only when it is signalled.
    const script = [
      '#!/bin/sh',
      `trap 'echo got-${sig}; exit 42' ${sig.slice(3)}`,
      'echo ready',
      // Bounded, so a launcher that fails to forward the signal leaves no orphan behind.
      'i=0; while [ $i -lt 200 ]; do i=$((i+1)); sleep 0.05; done',
      '',
    ].join('\n');
    const { launcher } = install(script);
    const child = spawn(process.execPath, [launcher], { stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '';
    await new Promise((resolve) => {
      child.stdout.on('data', (d) => {
        out += d;
        if (out.includes('ready')) resolve();
      });
    });
    child.kill(sig);
    const code = await new Promise((resolve) => child.on('exit', (c, s) => resolve(c ?? s)));
    assert.equal(code, 42);
    assert.match(out, new RegExp(`got-${sig}\\n$`));
  });
}

test('launcher_re_raises_the_signal_that_ended_the_binary', { skip }, async () => {
  const { launcher } = install('#!/bin/sh\nkill -KILL $$\n');
  const child = spawn(process.execPath, [launcher], { stdio: 'ignore' });
  const result = await new Promise((resolve) => child.on('exit', (c, s) => resolve({ c, s })));
  assert.deepEqual(result, { c: null, s: 'SIGKILL' });
});

test('package_json_declares_no_install_script_and_pins_the_platform_packages', () => {
  const pj = require('../package.json');
  assert.equal(pj.name, '@jcioriente/minuta');
  assert.equal(pj.license, 'Apache-2.0');
  assert.deepEqual(pj.bin, { minuta: 'bin/minuta.js' });
  assert.ok(pj.engines.node);
  for (const s of ['preinstall', 'install', 'postinstall']) assert.equal(pj.scripts?.[s], undefined);
  assert.deepEqual(
    Object.keys(pj.optionalDependencies).sort(),
    [
      '@jcioriente/minuta-darwin-arm64',
      '@jcioriente/minuta-darwin-x64',
      '@jcioriente/minuta-linux-arm64',
      '@jcioriente/minuta-linux-x64',
    ],
  );
});
