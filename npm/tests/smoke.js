"use strict";

const fs = require("fs");
const os = require("os");
const path = require("path");
const { spawnSync } = require("child_process");

const root = path.resolve(__dirname, "..", "..");
const cli = path.join(root, "npm", "bin", "zcli.js");
const temp = fs.mkdtempSync(path.join(os.tmpdir(), "zcli-npm-smoke-"));
const config = path.join(temp, "config.toml");
const db = path.join(temp, "zotero.sqlite");
const storage = path.join(temp, "storage");

fs.mkdirSync(storage, { recursive: true });

function run(args) {
  const result = spawnSync(process.execPath, [cli].concat(args), {
    cwd: root,
    encoding: "utf8",
  });
  if (result.status !== 0) {
    process.stderr.write(result.stdout || "");
    process.stderr.write(result.stderr || "");
    process.exit(result.status || 1);
  }
  return result.stdout;
}

run(["--help"]);
const stdout = run([
  "--config",
  config,
  "--db",
  db,
  "--storage",
  storage,
  "setup",
  "--defaults",
  "--dry-run",
]);

const parsed = JSON.parse(stdout);
if (parsed.ok !== true || parsed.dry_run !== true || parsed.wrote_config !== false) {
  throw new Error("unexpected zcli setup smoke output");
}

console.log("npm smoke ok");
