// Pure install logic — no UI. Emits InstallEvent objects so the caller
// (Ink UI in TTY mode, plain console in CI) can render them however it wants.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  unlinkSync,
} from "node:fs";
import { readdir } from "node:fs/promises";
import { request } from "node:https";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
export const ROOT = join(__dirname, "..");

export interface InstallStep {
  label: string;
  status: "running" | "ok" | "fail";
}

export type InstallEvent =
  | { kind: "version"; version: string }
  | { kind: "step:start"; index: number; label: string }
  | { kind: "step:end"; index: number; label: string; ok: boolean }
  | { kind: "progress"; ratio: number; suffix?: string }
  | { kind: "progress:clear" }
  | { kind: "error"; message: string }
  | { kind: "done" };

interface PlatformDescriptor {
  target: string;
  archive: "tar.gz" | "zip";
  binary: string;
}

function detectPlatform(): PlatformDescriptor {
  const platform = process.platform;
  const arch = process.arch;
  const isWin = platform === "win32";
  const target =
    platform === "linux" && arch === "x64"
      ? "x86_64-unknown-linux-gnu"
      : platform === "linux" && arch === "arm64"
        ? "aarch64-unknown-linux-gnu"
        : platform === "darwin" && arch === "x64"
          ? "x86_64-apple-darwin"
          : platform === "darwin" && arch === "arm64"
            ? "aarch64-apple-darwin"
            : isWin && arch === "x64"
              ? "x86_64-pc-windows-msvc"
              : isWin && arch === "arm64"
                ? "aarch64-pc-windows-msvc"
                : "";
  if (!target) throw new Error(`Unsupported platform/arch: ${platform}/${arch}`);
  return { target, archive: isWin ? "zip" : "tar.gz", binary: isWin ? "artui.exe" : "artui" };
}

async function findBinary(dir: string, name: string): Promise<string | null> {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      const hit = await findBinary(full, name);
      if (hit) return hit;
    } else if (entry.name === name) {
      return full;
    }
  }
  return null;
}

function unpack(archive: string, dest: string, type: "tar.gz" | "zip"): void {
  const result =
    type === "tar.gz"
      ? spawnSync("tar", ["-xzf", archive, "-C", dest], { stdio: "ignore" })
      : spawnSync(
          "powershell.exe",
          [
            "-NoProfile",
            "-Command",
            `Expand-Archive -Path '${archive}' -DestinationPath '${dest}' -Force`,
          ],
          { stdio: "ignore" },
        );
  if (result.status !== 0) {
    throw new Error(`${type === "tar.gz" ? "tar" : "Expand-Archive"} failed`);
  }
}

function downloadWithProgress(
  url: string,
  dest: string,
  emit: (event: InstallEvent) => void,
  redirects = 5,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const fetch = (current: string, hops: number) => {
      const req = request(current, { method: "GET" }, (res) => {
        const status = res.statusCode ?? 0;
        if ([301, 302, 307, 308].includes(status) && res.headers.location && hops > 0) {
          res.resume();
          fetch(res.headers.location, hops - 1);
          return;
        }
        if (status !== 200) {
          res.resume();
          reject(new Error(`HTTP ${status} for ${current}`));
          return;
        }
        const total = Number(res.headers["content-length"] ?? 0);
        let received = 0;
        const file = createWriteStream(dest);
        res.on("data", (chunk: Buffer) => {
          received += chunk.length;
          if (total > 0) {
            emit({
              kind: "progress",
              ratio: received / total,
              suffix: `${(received / 1024 / 1024).toFixed(1)} MiB`,
            });
          }
        });
        res.pipe(file);
        file.on("finish", () => {
          emit({ kind: "progress", ratio: 1, suffix: "done" });
          file.close(() => resolve());
        });
        file.on("error", reject);
      });
      req.on("error", reject);
      req.end();
    };
    fetch(url, redirects);
  });
}

export async function runInstall(
  version: string,
  emit: (event: InstallEvent) => void,
): Promise<void> {
  const repo = process.env.ARTUI_REPO ?? "lucasram20/artui";
  emit({ kind: "version", version });

  const platform = detectPlatform();
  const vendor = join(ROOT, "vendor");
  mkdirSync(vendor, { recursive: true });

  const asset = `artui-${version}-${platform.target}.${platform.archive}`;
  const url = `https://github.com/${repo}/releases/download/v${version}/${asset}`;
  const tmp = join(
    tmpdir(),
    `artui-${createHash("sha1").update(`${Date.now()}`).digest("hex").slice(0, 8)}-${asset}`,
  );

  let stepIndex = 0;
  const startStep = (label: string): number => {
    const i = stepIndex++;
    emit({ kind: "step:start", index: i, label });
    return i;
  };
  const finishStep = (i: number, label: string, ok: boolean) =>
    emit({ kind: "step:end", index: i, label, ok });

  // ── 1. Download ──
  const downloadStep = startStep(`Downloading ${platform.target}`);
  try {
    await downloadWithProgress(url, tmp, emit);
    finishStep(downloadStep, `Downloaded ${platform.target}`, true);
  } catch (err) {
    finishStep(downloadStep, `Download failed`, false);
    emit({
      kind: "error",
      message: `${(err as Error).message}\nFallback: ARTUI_FROM_SOURCE=1 curl -fsSL https://raw.githubusercontent.com/${repo}/main/scripts/install.sh | sh`,
    });
    return;
  }
  emit({ kind: "progress:clear" });

  // ── 2. Unpack ──
  const unpackStep = startStep("Extracting archive");
  try {
    unpack(tmp, vendor, platform.archive);
    finishStep(unpackStep, "Extracted archive", true);
  } catch (err) {
    finishStep(unpackStep, "Extract failed", false);
    emit({ kind: "error", message: (err as Error).message });
    return;
  }

  // ── 3. Place binary ──
  const placeStep = startStep("Installing binary");
  const binary = await findBinary(vendor, platform.binary);
  if (!binary) {
    finishStep(placeStep, `${platform.binary} missing from archive`, false);
    emit({ kind: "error", message: `Could not locate ${platform.binary} after extraction` });
    return;
  }
  const target = join(vendor, platform.binary);
  if (binary !== target) renameSync(binary, target);
  if (process.platform !== "win32") chmodSync(target, 0o755);
  if (existsSync(tmp)) unlinkSync(tmp);
  finishStep(placeStep, `Installed → ${target}`, true);

  emit({ kind: "done" });
}

export function readVersionFromPackage(): string {
  const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8")) as { version: string };
  return process.env.ARTUI_VERSION ?? pkg.version;
}

/** Plain console renderer for non-TTY / CI contexts. */
export async function runPlain(version: string): Promise<void> {
  await runInstall(version, (event) => {
    switch (event.kind) {
      case "version":
        process.stdout.write(`artui v${event.version} installer\n`);
        break;
      case "step:start":
        process.stdout.write(`• ${event.label}\n`);
        break;
      case "step:end":
        process.stdout.write(`${event.ok ? "✔" : "✖"} ${event.label}\n`);
        break;
      case "error":
        process.stderr.write(`✖ ${event.message}\n`);
        process.exitCode = 1;
        break;
      case "done":
        process.stdout.write(`✔ artui v${version} installed.\n`);
        break;
      default:
        break;
    }
  });
}
