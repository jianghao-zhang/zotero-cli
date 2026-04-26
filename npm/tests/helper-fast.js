"use strict";

const assert = require("assert");
const fs = require("fs");
const path = require("path");
const vm = require("vm");

const root = path.resolve(__dirname, "..", "..");
const bootstrap = path.join(root, "helper", "zcli-helper-zotero", "bootstrap.js");
const context = {
  console,
  TextDecoder,
  TextEncoder,
};

vm.createContext(context);
vm.runInContext(fs.readFileSync(bootstrap, "utf8"), context, {
  filename: bootstrap,
});

async function captureBatch(defaultDryRun, operation) {
  const calls = [];
  const helper = context.ZcliHelper;
  helper.token = "tok";
  helper.handle = async (request) => {
    calls.push(request);
    return { ok: true, dry_run: request.dry_run };
  };
  const result = await helper.batch({ operations: [operation] }, defaultDryRun, true);
  return { calls, result };
}

(async () => {
  let captured = await captureBatch(false, { op: "apply_tags" });
  assert.strictEqual(captured.calls[0].dry_run, false);
  assert.strictEqual(captured.result.results[0].dry_run, false);

  captured = await captureBatch(false, { op: "apply_tags", dry_run: true });
  assert.strictEqual(captured.calls[0].dry_run, true);

  captured = await captureBatch(true, { op: "apply_tags" });
  assert.strictEqual(captured.calls[0].dry_run, true);

  captured = await captureBatch(true, { op: "apply_tags", dry_run: false });
  assert.strictEqual(captured.calls[0].dry_run, true);

  console.log("helper fast tests ok");
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
