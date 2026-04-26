#!/usr/bin/env node
"use strict";

const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");
const { exeSuffix, nativeBinaryName, packageRoot } = require("../lib/platform");

function candidateBinaries(root) {
  const explicit = process.env.ZCLI_BINARY ? [process.env.ZCLI_BINARY] : [];
  return explicit.concat([
    path.join(root, "npm", "native", nativeBinaryName()),
    path.join(root, "target", "release", `zcli${exeSuffix()}`),
    path.join(root, "target", "debug", `zcli${exeSuffix()}`),
  ]);
}

function findBinary() {
  const root = packageRoot();
  for (const candidate of candidateBinaries(root)) {
    if (candidate && fs.existsSync(candidate)) {
      return candidate;
    }
  }
  return null;
}

const binary = findBinary();
if (!binary) {
  console.error(
    "zcli binary was not found. Reinstall the npm package, run `cargo build --release`, or set ZCLI_BINARY=/path/to/zcli.",
  );
  process.exit(127);
}

const result = spawnSync(binary, process.argv.slice(2), {
  stdio: "inherit",
  argv0: "zcli",
  env: {
    ...process.env,
    ZCLI_PACKAGE_ROOT: packageRoot(),
  },
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status === null ? 1 : result.status);
