'use strict';

const { resolveTarget } = require('./platform');

const INSTALL_GUIDE = 'https://github.com/JCIOriente/minuta/blob/main/docs/install.md';

// Finds the binary for this machine. Returns { ok: true, binary } or { ok: false, message }.
function locate({ platform, arch, resolve }) {
  if (platform === 'win32') {
    return {
      ok: false,
      message:
        'minuta needs WSL2 on Windows. Install it with `wsl --install`, then run ' +
        '`npm i -g @jcioriente/minuta` inside WSL2.\n' +
        `Guide: ${INSTALL_GUIDE}`,
    };
  }
  const target = resolveTarget(platform, arch);
  if (!target) {
    return {
      ok: false,
      message:
        `minuta has no prebuilt binary for ${platform}-${arch}.\n` +
        'Install it from source instead: cargo install --locked minuta',
    };
  }
  try {
    return { ok: true, binary: resolve(`${target.pkg}/bin/minuta`) };
  } catch (err) {
    if (err && err.code === 'MODULE_NOT_FOUND') {
      return {
        ok: false,
        message:
          `The package ${target.pkg} is not installed, so there is no minuta binary for this machine.\n` +
          'Reinstall without --omit=optional, on the same OS this node_modules was installed on: ' +
          'npm i -g @jcioriente/minuta',
      };
    }
    throw err;
  }
}

module.exports = { locate };
