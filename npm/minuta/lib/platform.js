'use strict';

// The one place that maps a Node platform to its package and Rust target.
// Keep in step with docs/specs/2026-11-03-npm-distribution.md.
const TARGETS = [
  { platform: 'darwin', arch: 'arm64', pkg: '@jcioriente/minuta-darwin-arm64', rustTarget: 'aarch64-apple-darwin' },
  { platform: 'darwin', arch: 'x64', pkg: '@jcioriente/minuta-darwin-x64', rustTarget: 'x86_64-apple-darwin' },
  { platform: 'linux', arch: 'x64', pkg: '@jcioriente/minuta-linux-x64', rustTarget: 'x86_64-unknown-linux-musl' },
  { platform: 'linux', arch: 'arm64', pkg: '@jcioriente/minuta-linux-arm64', rustTarget: 'aarch64-unknown-linux-musl' },
];

function resolveTarget(platform, arch) {
  const t = TARGETS.find((x) => x.platform === platform && x.arch === arch);
  return t ? { pkg: t.pkg, rustTarget: t.rustTarget } : null;
}

module.exports = { TARGETS, resolveTarget };
