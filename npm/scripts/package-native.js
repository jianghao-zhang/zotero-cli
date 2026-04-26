"use strict";

const fs = require("fs");
const path = require("path");
const { exeSuffix, nativeBinaryName, packageRoot } = require("../lib/platform");

const root = packageRoot();
const releaseBinary = path.join(root, "target", "release", `zcli${exeSuffix()}`);
const nativeDir = path.join(root, "npm", "native");
const nativeBinary = path.join(nativeDir, nativeBinaryName());

if (!fs.existsSync(releaseBinary)) {
  console.error(`release binary not found: ${releaseBinary}`);
  console.error("Run `cargo build --release --bin zcli` first.");
  process.exit(1);
}

fs.mkdirSync(nativeDir, { recursive: true });
fs.copyFileSync(releaseBinary, nativeBinary);
if (process.platform !== "win32") {
  fs.chmodSync(nativeBinary, 0o755);
}
console.log(`packaged ${nativeBinary}`);
