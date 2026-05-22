// Tiny launcher — spawn the platform binary downloaded by postinstall.ts.
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const isWin = process.platform === "win32";
const binary = join(__dirname, "..", "vendor", isWin ? "artui.exe" : "artui");

if (!existsSync(binary)) {
  process.stderr.write(`artui binary not found at ${binary}\n`);
  process.stderr.write("Try reinstalling: npm install -g artui\n");
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
