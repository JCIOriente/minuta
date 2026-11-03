#!/usr/bin/env node
'use strict';

const { spawn } = require('node:child_process');
const os = require('node:os');
const { locate } = require('../lib/launcher');

const found = locate({ platform: process.platform, arch: process.arch, resolve: require.resolve });
if (!found.ok) {
  console.error(found.message);
  process.exit(1);
}

const child = spawn(found.binary, process.argv.slice(2), { stdio: 'inherit' });

// Same signals minuta itself stops on (src/main.rs, ctrlc `termination`).
for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(sig, () => child.kill(sig));
}

child.on('error', (err) => {
  console.error(`could not start ${found.binary}: ${err.message}`);
  process.exit(1);
});

child.on('exit', (code, signal) => {
  if (signal) {
    // Drop our handlers so the re-raised signal ends this process the way it ended the binary.
    for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) process.removeAllListeners(sig);
    process.kill(process.pid, signal);
    // A signal with no default action would leave us running; fall back to the shell convention.
    process.exit(128 + (os.constants.signals[signal] ?? 0));
  }
  process.exit(code);
});
