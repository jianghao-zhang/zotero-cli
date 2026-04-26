"use strict";

const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");
const { exeSuffix, nativeBinaryName, packageRoot } = require("../lib/platform");

const root = packageRoot();
const nativeDir = path.join(root, "npm", "native");
const nativeBinary = path.join(nativeDir, nativeBinaryName());
const releaseBinary = path.join(root, "target", "release", `zcli${exeSuffix()}`);

if (process.env.ZOTERO_CLI_SKIP_POSTINSTALL === "1") {
  process.exit(0);
}

function newestMtimeMs(target) {
  if (!fs.existsSync(target)) return 0;
  const stat = fs.statSync(target);
  if (!stat.isDirectory()) return stat.mtimeMs;
  let newest = stat.mtimeMs;
  for (const entry of fs.readdirSync(target)) {
    newest = Math.max(newest, newestMtimeMs(path.join(target, entry)));
  }
  return newest;
}

const sourceMtime = Math.max(
  newestMtimeMs(path.join(root, "Cargo.toml")),
  newestMtimeMs(path.join(root, "Cargo.lock")),
  newestMtimeMs(path.join(root, "src")),
  newestMtimeMs(path.join(root, "helper")),
);
const nativeMtime = fs.existsSync(nativeBinary) ? fs.statSync(nativeBinary).mtimeMs : 0;

if (nativeMtime >= sourceMtime) {
  process.exit(0);
}

const cargoCheck = spawnSync("cargo", ["--version"], {
  cwd: root,
  stdio: "ignore",
});

if (cargoCheck.status !== 0) {
  console.warn(
    "zotero-cli: no prebuilt zcli binary was packaged and Cargo was not found. Install Rust or set ZCLI_BINARY before running zcli.",
  );
  process.exit(0);
}

console.warn("zotero-cli: building local Rust binary for this platform...");
const build = spawnSync("cargo", ["build", "--release", "--bin", "zcli"], {
  cwd: root,
  stdio: "inherit",
});

if (build.status !== 0) {
  console.warn("zotero-cli: cargo build failed; zcli will use any existing target/debug or target/release binary if present.");
  process.exit(0);
}

fs.mkdirSync(nativeDir, { recursive: true });
fs.copyFileSync(releaseBinary, nativeBinary);
if (process.platform !== "win32") {
  fs.chmodSync(nativeBinary, 0o755);
}
