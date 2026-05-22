#!/usr/bin/env node
// Tiny launcher — exec the platform binary downloaded by postinstall.js.
"use strict";

const { spawn } = require("node:child_process");
const path = require("node:path");
const fs = require("node:fs");

const isWin = process.platform === "win32";
const binary = path.join(__dirname, "..", "vendor", isWin ? "artui.exe" : "artui");

if (!fs.existsSync(binary)) {
  console.error(`artui binary not found at ${binary}`);
  console.error("Try reinstalling: npm install -g artui");
  process.exit(127);
}

const child = spawn(binary, process.argv.slice(2), { stdio: "inherit" });
child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exit(code ?? 0);
  }
});
