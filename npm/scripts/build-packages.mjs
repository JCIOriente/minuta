#!/usr/bin/env node
// Writes the five npm packages for one release into an output directory:
// one package per platform holding the binary, and the main launcher package.
//
//   node npm/scripts/build-packages.mjs --version 0.8.0 --out dist \
//     --binary aarch64-apple-darwin=path/to/minuta --binary ... (one per target)
//
// Prints the package directories, one per line, in publish order (main package last).
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const { TARGETS } = createRequire(import.meta.url)('../minuta/lib/platform.js');

const MAIN_NAME = '@jcioriente/minuta';
const REPOSITORY = { type: 'git', url: 'git+https://github.com/JCIOriente/minuta.git' };
const HOMEPAGE = 'https://github.com/JCIOriente/minuta';

const USAGE =
  'usage: build-packages.mjs --version <X.Y.Z> --out <dir> [--cargo-toml <file>] [--main-src <dir>] ' +
  '--binary <rust-target>=<path> (once per target)';

// Version of the `minuta` crate: the `version` key of the [package] table.
function cargoVersion(cargoToml) {
  const text = fs.readFileSync(cargoToml, 'utf8');
  const table = text.split(/^\[/m).find((t) => t.startsWith('package]'));
  const m = table && table.match(/^version\s*=\s*"([^"]+)"/m);
  if (!m) throw new Error(`no [package] version in ${cargoToml}`);
  return m[1];
}

// The launcher package source: bin/, lib/ and a parsable package.json. Returns its manifest.
function readMainSource(mainSrc) {
  for (const entry of ['bin', 'lib', 'package.json']) {
    const p = path.join(mainSrc, entry);
    if (!fs.existsSync(p)) throw new Error(`main package source is missing ${p}`);
  }
  const file = path.join(mainSrc, 'package.json');
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (err) {
    throw new Error(`cannot parse ${file}: ${err.message}`, { cause: err });
  }
}

// Everything that can be wrong is found here, before the first file is written.
// Returns the parsed main package.json.
function checkInputs({ version, cargoToml, binaries, outDir, mainSrc }) {
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error(`version ${version} is not X.Y.Z`);
  const crate = cargoVersion(cargoToml);
  if (crate !== version) throw new Error(`version ${version} does not match Cargo.toml version ${crate}`);
  const known = new Set(TARGETS.map((t) => t.rustTarget));
  for (const t of Object.keys(binaries)) {
    if (!known.has(t)) throw new Error(`unknown target ${t}`);
  }
  for (const { rustTarget } of TARGETS) {
    const p = binaries[rustTarget];
    if (p === undefined) throw new Error(`no binary given for ${rustTarget}`);
    const stat = fs.statSync(p, { throwIfNoEntry: false });
    if (!stat) throw new Error(`binary for ${rustTarget} does not exist: ${p}`);
    if (!stat.isFile()) throw new Error(`binary for ${rustTarget} is not a file: ${p}`);
  }
  const main = readMainSource(mainSrc);
  if (fs.existsSync(outDir) && fs.readdirSync(outDir).length > 0) {
    throw new Error(`output directory is not empty: ${outDir}`);
  }
  return main;
}

// `version` may carry the tag's leading "v". Returns [{ name, dir }] in publish order.
export function buildPackages({ version, cargoToml, binaries, outDir, mainSrc }) {
  version = version.replace(/^v/, '');
  const main = checkInputs({ version, cargoToml, binaries, outDir, mainSrc });

  const published = [];
  for (const { platform, arch, pkg, rustTarget } of TARGETS) {
    const dir = path.join(outDir, pkg.split('/')[1]);
    fs.mkdirSync(path.join(dir, 'bin'), { recursive: true });
    const manifest = {
      name: pkg,
      version,
      description: `The minuta binary for ${platform}-${arch}`,
      license: 'Apache-2.0',
      repository: REPOSITORY,
      homepage: HOMEPAGE,
      os: [platform],
      cpu: [arch],
      files: ['bin/'],
    };
    fs.writeFileSync(path.join(dir, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`);
    const bin = path.join(dir, 'bin', 'minuta');
    fs.copyFileSync(binaries[rustTarget], bin);
    fs.chmodSync(bin, 0o755);
    published.push({ name: pkg, dir });
  }

  const mainDir = path.join(outDir, 'minuta');
  fs.mkdirSync(mainDir, { recursive: true });
  for (const sub of ['bin', 'lib']) {
    fs.cpSync(path.join(mainSrc, sub), path.join(mainDir, sub), { recursive: true });
  }
  fs.chmodSync(path.join(mainDir, 'bin', 'minuta.js'), 0o755);
  main.version = version;
  // Exact versions, no range: a launcher must never run a binary of another version.
  main.optionalDependencies = Object.fromEntries(
    TARGETS.map((t) => [t.pkg, version]).sort(([a], [b]) => a.localeCompare(b)),
  );
  fs.writeFileSync(path.join(mainDir, 'package.json'), `${JSON.stringify(main, null, 2)}\n`);
  published.push({ name: MAIN_NAME, dir: mainDir });
  return published;
}

function parseArgs(argv) {
  const given = new Set();
  const opts = { cargoToml: path.join(here, '..', '..', 'Cargo.toml'), mainSrc: path.join(here, '..', 'minuta'), binaries: {} };
  for (let i = 0; i < argv.length; i += 2) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (value === undefined) throw new UsageError(`${flag} needs a value`);
    if (flag === '--binary') {
      const eq = value.indexOf('=');
      if (eq < 1) throw new UsageError(`--binary needs <rust-target>=<path>, got ${value}`);
      const target = value.slice(0, eq);
      if (target in opts.binaries) throw new UsageError(`duplicate --binary for ${target}`);
      opts.binaries[target] = value.slice(eq + 1);
      continue;
    }
    const key = SINGLE_FLAGS[flag];
    if (key === undefined) throw new UsageError(`unknown flag ${flag}`);
    // The defaults count as unset: only a flag given twice on the command line is a duplicate.
    if (given.has(flag)) throw new UsageError(`duplicate ${flag}`);
    given.add(flag);
    opts[key] = value;
  }
  for (const [key, flag] of [['version', '--version'], ['outDir', '--out']]) {
    if (opts[key] === undefined) throw new UsageError(`missing ${flag}`);
  }
  return opts;
}

class UsageError extends Error {}

const SINGLE_FLAGS = { '--version': 'version', '--out': 'outDir', '--cargo-toml': 'cargoToml', '--main-src': 'mainSrc' };

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    for (const { dir } of buildPackages(parseArgs(process.argv.slice(2)))) console.log(dir);
  } catch (err) {
    console.error(`build-packages: ${err.message}`);
    if (err instanceof UsageError) {
      console.error(USAGE);
      process.exit(2);
    }
    process.exit(1);
  }
}
