import test, { after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { buildPackages } from '../build-packages.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const SCRIPT = path.join(here, '..', 'build-packages.mjs');
const MAIN_SRC = path.join(here, '..', '..', 'minuta');
const { TARGETS } = createRequire(import.meta.url)('../../minuta/lib/platform.js');

const EXPECTED = [
  ['aarch64-apple-darwin', '@jcioriente/minuta-darwin-arm64', 'darwin', 'arm64'],
  ['x86_64-apple-darwin', '@jcioriente/minuta-darwin-x64', 'darwin', 'x64'],
  ['x86_64-unknown-linux-musl', '@jcioriente/minuta-linux-x64', 'linux', 'x64'],
  ['aarch64-unknown-linux-musl', '@jcioriente/minuta-linux-arm64', 'linux', 'arm64'],
];

const made = [];
function tmp() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'minuta-build-'));
  made.push(dir);
  return dir;
}
after(() => {
  for (const dir of made) fs.rmSync(dir, { recursive: true, force: true });
});

// Fake binaries with a mode that is NOT executable, to prove the script sets the mode itself.
function fixture({ cargoVersion = '1.2.3' } = {}) {
  const root = tmp();
  const cargoToml = path.join(root, 'Cargo.toml');
  fs.writeFileSync(
    cargoToml,
    `[package]\nname = "minuta"\nversion = "${cargoVersion}"\nedition = "2021"\n\n[dependencies]\nclap = { version = "4.0.0" }\n`,
  );
  const binaries = {};
  for (const [rustTarget] of EXPECTED) {
    const p = path.join(root, `bin-${rustTarget}`);
    fs.writeFileSync(p, `binary for ${rustTarget}\n`, { mode: 0o644 });
    binaries[rustTarget] = p;
  }
  return { root, cargoToml, binaries, outDir: path.join(root, 'out') };
}

const read = (p) => JSON.parse(fs.readFileSync(p, 'utf8'));
const run = (f, overrides = {}) =>
  buildPackages({ version: '1.2.3', mainSrc: MAIN_SRC, ...f, ...overrides });

test('the_script_knows_the_same_four_targets_as_the_launcher', () => {
  assert.deepEqual(
    TARGETS.map((t) => [t.rustTarget, t.pkg, t.platform, t.arch]),
    EXPECTED,
  );
});

for (const [rustTarget, pkg, platform, arch] of EXPECTED) {
  test(`writes_platform_package_with_os_and_cpu_${platform}_${arch}`, () => {
    const f = fixture();
    run(f);
    const dir = path.join(f.outDir, pkg.split('/')[1]);
    assert.deepEqual(read(path.join(dir, 'package.json')), {
      name: pkg,
      version: '1.2.3',
      description: `The minuta binary for ${platform}-${arch}`,
      license: 'Apache-2.0',
      repository: { type: 'git', url: 'git+https://github.com/JCIOriente/minuta.git' },
      homepage: 'https://github.com/JCIOriente/minuta',
      os: [platform],
      cpu: [arch],
      files: ['bin/'],
    });
    assert.equal(fs.readFileSync(path.join(dir, 'bin', 'minuta'), 'utf8'), `binary for ${rustTarget}\n`);
    assert.deepEqual(fs.readdirSync(dir).sort(), ['bin', 'package.json']);
    assert.deepEqual(fs.readdirSync(path.join(dir, 'bin')), ['minuta']);
  });

  test(`binary_is_mode_0755_${platform}_${arch}`, () => {
    const f = fixture();
    run(f);
    const mode = fs.statSync(path.join(f.outDir, pkg.split('/')[1], 'bin', 'minuta')).mode & 0o777;
    assert.equal(mode.toString(8), '755');
  });
}

test('main_package_gets_the_version_and_exact_optional_dependencies', () => {
  const f = fixture();
  run(f);
  const pj = read(path.join(f.outDir, 'minuta', 'package.json'));
  assert.equal(pj.name, '@jcioriente/minuta');
  assert.equal(pj.version, '1.2.3');
  assert.deepEqual(pj.optionalDependencies, {
    '@jcioriente/minuta-darwin-arm64': '1.2.3',
    '@jcioriente/minuta-darwin-x64': '1.2.3',
    '@jcioriente/minuta-linux-arm64': '1.2.3',
    '@jcioriente/minuta-linux-x64': '1.2.3',
  });
  assert.deepEqual(Object.keys(pj.optionalDependencies), [
    '@jcioriente/minuta-darwin-arm64',
    '@jcioriente/minuta-darwin-x64',
    '@jcioriente/minuta-linux-arm64',
    '@jcioriente/minuta-linux-x64',
  ]);
  // The rest of the source package.json is kept as it is.
  const src = read(path.join(MAIN_SRC, 'package.json'));
  assert.deepEqual({ ...pj, version: src.version, optionalDependencies: src.optionalDependencies }, src);
});

test('main_package_ships_bin_and_lib_and_not_the_tests', () => {
  const f = fixture();
  run(f);
  const dir = path.join(f.outDir, 'minuta');
  assert.deepEqual(fs.readdirSync(dir).sort(), ['bin', 'lib', 'package.json']);
  assert.deepEqual(fs.readdirSync(path.join(dir, 'bin')), ['minuta.js']);
  assert.deepEqual(fs.readdirSync(path.join(dir, 'lib')).sort(), ['launcher.js', 'platform.js']);
});

test('main_launcher_is_mode_0755_even_when_the_source_is_not', () => {
  const f = fixture();
  const mainSrc = path.join(f.root, 'main-src');
  fs.cpSync(MAIN_SRC, mainSrc, { recursive: true });
  fs.chmodSync(path.join(mainSrc, 'bin', 'minuta.js'), 0o644);
  run(f, { mainSrc });
  const mode = fs.statSync(path.join(f.outDir, 'minuta', 'bin', 'minuta.js')).mode & 0o777;
  assert.equal(mode.toString(8), '755');
});

test('the_source_package_is_not_modified', () => {
  const before = fs.readFileSync(path.join(MAIN_SRC, 'package.json'), 'utf8');
  run(fixture());
  assert.equal(fs.readFileSync(path.join(MAIN_SRC, 'package.json'), 'utf8'), before);
});

test('returns_the_publish_order_with_the_main_package_last', () => {
  const f = fixture();
  const result = run(f);
  assert.deepEqual(
    result.map((r) => [r.name, r.dir]),
    [
      ['@jcioriente/minuta-darwin-arm64', path.join(f.outDir, 'minuta-darwin-arm64')],
      ['@jcioriente/minuta-darwin-x64', path.join(f.outDir, 'minuta-darwin-x64')],
      ['@jcioriente/minuta-linux-x64', path.join(f.outDir, 'minuta-linux-x64')],
      ['@jcioriente/minuta-linux-arm64', path.join(f.outDir, 'minuta-linux-arm64')],
      ['@jcioriente/minuta', path.join(f.outDir, 'minuta')],
    ],
  );
});

test('rejects_version_not_matching_cargo_toml', () => {
  const f = fixture({ cargoVersion: '1.2.4' });
  assert.throws(
    () => run(f),
    { message: 'version 1.2.3 does not match Cargo.toml version 1.2.4' },
  );
  assert.equal(fs.existsSync(f.outDir), false, 'nothing is written on a mismatch');
});

test('rejects_a_cargo_version_that_only_starts_with_the_given_version', () => {
  const f = fixture({ cargoVersion: '1.2.30' });
  assert.throws(() => run(f), { message: 'version 1.2.3 does not match Cargo.toml version 1.2.30' });
});

test('accepts_a_v_prefixed_tag_as_the_version', () => {
  const f = fixture();
  assert.doesNotThrow(() => run(f, { version: 'v1.2.3' }));
  assert.equal(read(path.join(f.outDir, 'minuta', 'package.json')).version, '1.2.3');
});

for (const bad of ['1.2', '1.2.3-rc.1', '1.2.3.4', 'x1.2.3', 'vv1.2.3', '']) {
  test(`rejects_a_version_that_is_not_x_y_z_${JSON.stringify(bad)}`, () => {
    const f = fixture({ cargoVersion: bad.replace(/^v/, '') });
    const shown = bad.replace(/^v/, '');
    assert.throws(() => run(f, { version: bad }), { message: `version ${shown} is not X.Y.Z` });
    assert.equal(fs.existsSync(f.outDir), false);
  });
}

test('reads_the_version_from_the_package_table_not_from_a_decoy_before_or_after_it', () => {
  const f = fixture();
  fs.writeFileSync(
    f.cargoToml,
    '[dependencies.foo]\nversion = "9.9.9"\n\n[package]\nname = "minuta"\nversion = "1.2.3"\n\n' +
      '[profile.release]\nversion = "8.8.8"\n[workspace.package]\nversion = "7.7.7"\n',
  );
  assert.doesNotThrow(() => run(f));
});

test('reads_the_version_key_not_a_longer_key_that_ends_in_version', () => {
  const f = fixture();
  fs.writeFileSync(
    f.cargoToml,
    '[workspace.package]\nversion = "7.7.7"\n\n[package]\nname = "minuta"\nrust-version = "9.9.9"\n' +
      'keywords = ["a"]\nversion = "1.2.3"\n',
  );
  assert.doesNotThrow(() => run(f));
});

test('a_decoy_version_outside_the_package_table_is_not_the_crate_version', () => {
  const f = fixture();
  fs.writeFileSync(f.cargoToml, '[package]\nname = "minuta"\n\n[dependencies.foo]\nversion = "1.2.3"\n');
  assert.throws(() => run(f), { message: `no [package] version in ${f.cargoToml}` });
});

test('rejects_cargo_toml_without_a_package_version', () => {
  const f = fixture();
  fs.writeFileSync(f.cargoToml, '[dependencies]\nclap = "4"\n');
  assert.throws(() => run(f), { message: `no [package] version in ${f.cargoToml}` });
});

for (const [rustTarget] of EXPECTED) {
  test(`rejects_missing_binary_${rustTarget}`, () => {
    const f = fixture();
    const gone = path.join(f.root, 'does-not-exist');
    assert.throws(
      () => run(f, { binaries: { ...f.binaries, [rustTarget]: gone } }),
      { message: `binary for ${rustTarget} does not exist: ${gone}` },
    );
    assert.equal(fs.existsSync(f.outDir), false, 'nothing is written when a binary is missing');
  });

  test(`rejects_binary_that_is_a_directory_${rustTarget}`, () => {
    const f = fixture();
    assert.throws(
      () => run(f, { binaries: { ...f.binaries, [rustTarget]: f.root } }),
      { message: `binary for ${rustTarget} is not a file: ${f.root}` },
    );
    assert.equal(fs.existsSync(f.outDir), false);
  });

  test(`rejects_no_binary_given_for_${rustTarget}`, () => {
    const f = fixture();
    const { [rustTarget]: _omitted, ...rest } = f.binaries;
    assert.throws(() => run(f, { binaries: rest }), { message: `no binary given for ${rustTarget}` });
    assert.equal(fs.existsSync(f.outDir), false);
  });

  test(`rejects_an_empty_binary_${rustTarget}`, () => {
    const f = fixture();
    fs.writeFileSync(f.binaries[rustTarget], '');
    assert.throws(
      () => run(f),
      { message: `binary for ${rustTarget} is empty: ${f.binaries[rustTarget]}` },
    );
    assert.equal(fs.existsSync(f.outDir), false);
  });
}

test('rejects_a_binary_for_an_unknown_target', () => {
  const f = fixture();
  assert.throws(
    () => run(f, { binaries: { ...f.binaries, 'x86_64-pc-windows-msvc': f.binaries['x86_64-apple-darwin'] } }),
    { message: 'unknown target x86_64-pc-windows-msvc' },
  );
  assert.equal(fs.existsSync(f.outDir), false, 'nothing is written for an unknown target');
});

test('refuses_to_write_into_a_non_empty_output_directory', () => {
  const f = fixture();
  fs.mkdirSync(f.outDir);
  fs.writeFileSync(path.join(f.outDir, 'stale'), 'x');
  assert.throws(() => run(f), { message: `output directory is not empty: ${f.outDir}` });
  assert.deepEqual(fs.readdirSync(f.outDir), ['stale'], 'the directory is left as it was');
});

for (const [removed, reported] of [
  ['bin', 'bin/minuta.js'],
  ['bin/minuta.js', 'bin/minuta.js'],
  ['lib', 'lib'],
  ['package.json', 'package.json'],
]) {
  test(`rejects_a_main_source_without_${removed.replace('/', '_')}_before_writing_anything`, () => {
    const f = fixture();
    const mainSrc = path.join(f.root, 'main-src');
    fs.cpSync(MAIN_SRC, mainSrc, { recursive: true });
    fs.rmSync(path.join(mainSrc, removed), { recursive: true });
    assert.throws(() => run(f, { mainSrc }), { message: `main package source is missing ${path.join(mainSrc, reported)}` });
    assert.equal(fs.existsSync(f.outDir), false);
  });
}

test('rejects_a_main_source_where_lib_is_a_file', () => {
  const f = fixture();
  const mainSrc = path.join(f.root, 'main-src');
  fs.cpSync(MAIN_SRC, mainSrc, { recursive: true });
  fs.rmSync(path.join(mainSrc, 'lib'), { recursive: true });
  fs.writeFileSync(path.join(mainSrc, 'lib'), 'x');
  assert.throws(() => run(f, { mainSrc }), { message: `main package source is missing ${path.join(mainSrc, 'lib')}` });
  assert.equal(fs.existsSync(f.outDir), false);
});

test('rejects_an_unparsable_main_package_json_naming_the_file_before_writing', () => {
  const f = fixture();
  const mainSrc = path.join(f.root, 'main-src');
  fs.cpSync(MAIN_SRC, mainSrc, { recursive: true });
  fs.writeFileSync(path.join(mainSrc, 'package.json'), '{ not json');
  assert.throws(() => run(f, { mainSrc }), (err) => {
    assert.match(err.message, new RegExp(`^cannot parse ${path.join(mainSrc, 'package.json')}: `));
    assert.ok(err.cause instanceof SyntaxError);
    return true;
  });
  assert.equal(fs.existsSync(f.outDir), false);
});

test('cli_builds_from_flags_and_prints_one_package_dir_per_line', () => {
  const f = fixture();
  const args = ['--version', '1.2.3', '--cargo-toml', f.cargoToml, '--out', f.outDir];
  for (const [t, p] of Object.entries(f.binaries)) args.push('--binary', `${t}=${p}`);
  const r = spawnSync(process.execPath, [SCRIPT, ...args], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.stderr, '');
  assert.deepEqual(
    r.stdout.trim().split('\n'),
    [
      'minuta-darwin-arm64',
      'minuta-darwin-x64',
      'minuta-linux-x64',
      'minuta-linux-arm64',
      'minuta',
    ].map((d) => path.join(f.outDir, d)),
  );
});

test('cli_exits_1_with_the_message_on_stderr_when_the_version_mismatches', () => {
  const f = fixture({ cargoVersion: '9.9.9' });
  const args = ['--version', '1.2.3', '--cargo-toml', f.cargoToml, '--out', f.outDir];
  for (const [t, p] of Object.entries(f.binaries)) args.push('--binary', `${t}=${p}`);
  const r = spawnSync(process.execPath, [SCRIPT, ...args], { encoding: 'utf8' });
  assert.equal(r.status, 1);
  assert.equal(r.stdout, '');
  assert.equal(r.stderr, 'build-packages: version 1.2.3 does not match Cargo.toml version 9.9.9\n');
});

test('cli_exits_2_on_a_missing_required_flag', () => {
  const r = spawnSync(process.execPath, [SCRIPT, '--version', '1.2.3'], { encoding: 'utf8' });
  assert.equal(r.status, 2);
  assert.match(r.stderr, /^build-packages: missing --out\nusage: /);
});

function cliArgs(f, extra = []) {
  const args = ['--version', '1.2.3', '--cargo-toml', f.cargoToml, '--out', f.outDir];
  for (const [t, p] of Object.entries(f.binaries)) args.push('--binary', `${t}=${p}`);
  return [...args, ...extra];
}

const USAGE_LINE =
  'usage: build-packages.mjs --version <X.Y.Z> --out <dir> [--cargo-toml <file>] [--main-src <dir>] ' +
  '--binary <rust-target>=<path> (once per target)\n';

const CLI_USAGE_ERRORS = [
  ['unknown_flag', ['--bogus', 'x'], 'unknown flag --bogus'],
  ['flag_without_value', ['--main-src'], '--main-src needs a value'],
  ['binary_without_equals', ['--binary', 'foo'], '--binary needs <rust-target>=<path>, got foo'],
  ['binary_with_empty_target', ['--binary', '=x'], '--binary needs <rust-target>=<path>, got =x'],
  ['duplicate_version', ['--version', '1.2.3'], 'duplicate --version'],
  ['duplicate_out', ['--out', 'elsewhere'], 'duplicate --out'],
  ['duplicate_cargo_toml', ['--cargo-toml', 'elsewhere'], 'duplicate --cargo-toml'],
  ['duplicate_main_src', ['--main-src', 'a', '--main-src', 'b'], 'duplicate --main-src'],
  ['duplicate_binary_target', ['--binary', 'x86_64-apple-darwin=other'], 'duplicate --binary for x86_64-apple-darwin'],
];
for (const [name, extra, message] of CLI_USAGE_ERRORS) {
  test(`cli_exits_2_on_${name}`, () => {
    const f = fixture();
    const r = spawnSync(process.execPath, [SCRIPT, ...cliArgs(f, extra)], { encoding: 'utf8' });
    assert.equal(r.status, 2);
    assert.equal(r.stdout, '');
    assert.equal(r.stderr, `build-packages: ${message}\n${USAGE_LINE}`);
    assert.equal(fs.existsSync(f.outDir), false);
  });
}

test('cli_exits_2_when_version_is_missing', () => {
  const r = spawnSync(process.execPath, [SCRIPT, '--out', 'x'], { encoding: 'utf8' });
  assert.equal(r.status, 2);
  assert.equal(r.stderr, `build-packages: missing --version\n${USAGE_LINE}`);
});

test('cli_main_src_flag_selects_the_launcher_source', () => {
  const f = fixture();
  const mainSrc = path.join(f.root, 'main-src');
  fs.cpSync(MAIN_SRC, mainSrc, { recursive: true });
  fs.writeFileSync(path.join(mainSrc, 'lib', 'marker.js'), '// marker\n');
  const r = spawnSync(process.execPath, [SCRIPT, ...cliArgs(f, ['--main-src', mainSrc])], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  assert.ok(fs.existsSync(path.join(f.outDir, 'minuta', 'lib', 'marker.js')));
});

test('cli_exits_2_on_an_empty_out_instead_of_writing_into_the_current_directory', () => {
  const f = fixture();
  const args = ['--version', '1.2.3', '--cargo-toml', f.cargoToml, '--out', ''];
  for (const [t, p] of Object.entries(f.binaries)) args.push('--binary', `${t}=${p}`);
  const r = spawnSync(process.execPath, [SCRIPT, ...args], { encoding: 'utf8', cwd: f.root });
  assert.equal(r.status, 2);
  assert.equal(r.stderr, `build-packages: --out needs a value\n${USAGE_LINE}`);
  assert.deepEqual(fs.readdirSync(f.root).filter((n) => n.startsWith('minuta')), []);
});

for (const key of ['constructor', '__proto__', 'toString']) {
  test(`cli_reports_${key}_as_an_unknown_target_not_a_duplicate`, () => {
    const f = fixture();
    const r = spawnSync(process.execPath, [SCRIPT, ...cliArgs(f, ['--binary', `${key}=/x`])], { encoding: 'utf8' });
    assert.equal(r.status, 1);
    assert.equal(r.stderr, `build-packages: unknown target ${key}\n`);
  });
}

test('cli_reads_Cargo_toml_from_the_repository_by_default', () => {
  const f = fixture();
  const repoCargo = fs.readFileSync(path.join(here, '..', '..', '..', 'Cargo.toml'), 'utf8');
  const version = repoCargo.match(/^version\s*=\s*"([^"]+)"/m)[1];
  const args = ['--version', version, '--out', f.outDir];
  for (const [t, p] of Object.entries(f.binaries)) args.push('--binary', `${t}=${p}`);
  const r = spawnSync(process.execPath, [SCRIPT, ...args], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(read(path.join(f.outDir, 'minuta', 'package.json')).version, version);
});
