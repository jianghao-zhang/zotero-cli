"use strict";

const path = require("path");

function exeSuffix() {
  return process.platform === "win32" ? ".exe" : "";
}

function platformTriple() {
  const platform = process.platform;
  const arch = process.arch;
  const mappedPlatform =
    platform === "darwin"
      ? "darwin"
      : platform === "linux"
        ? "linux"
        : platform === "win32"
          ? "win32"
          : platform;
  const mappedArch =
    arch === "x64"
      ? "x64"
      : arch === "arm64"
        ? "arm64"
        : arch;
  return `${mappedPlatform}-${mappedArch}`;
}

function packageRoot() {
  return path.resolve(__dirname, "..", "..");
}

function nativeBinaryName() {
  return `zcli-${platformTriple()}${exeSuffix()}`;
}

module.exports = {
  exeSuffix,
  nativeBinaryName,
  packageRoot,
  platformTriple,
};
