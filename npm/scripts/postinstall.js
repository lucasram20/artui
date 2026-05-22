#!/usr/bin/env node
// Postinstall: download the matching artui release binary from GitHub Releases
// and unpack it into ../vendor. Mirrors the `turbo` / `esbuild` pattern.
"use strict";

const fs = require("node:fs");
const https = require("node:https");
const path = require("node:path");
const { execFileSync } = require("node:child_process");
const tar = (() => {
  try {
    // Optional dep — fall back to system `tar` if missing.
    return require("tar");
  } catch {
    return null;
  }
})();

const REPO = process.env.ARTUI_REPO || "lucasram20/artui";
const VERSION = process.env.ARTUI_VERSION || require("../package.json").version;

const PLATFORM_TARGET = (() => {
  const platform = process.platform;
  const arch = process.arch;
  if (platform === "linux" && arch === "x64") return "x86_64-unknown-linux-gnu";
  if (platform === "linux" && arch === "arm64") return "aarch64-unknown-linux-gnu";
  if (platform === "darwin" && arch === "x64") return "x86_64-apple-darwin";
  if (platform === "darwin" && arch === "arm64") return "aarch64-apple-darwin";
  if (platform === "win32" && arch === "x64") return "x86_64-pc-windows-msvc";
  if (platform === "win32" && arch === "arm64") return "aarch64-pc-windows-msvc";
  throw new Error(`Unsupported platform/arch: ${platform}/${arch}`);
})();

const isWin = process.platform === "win32";
const ARCHIVE_EXT = isWin ? "zip" : "tar.gz";
const ASSET_NAME = `artui-${VERSION}-${PLATFORM_TARGET}.${ARCHIVE_EXT}`;
const ASSET_URL = `https://github.com/${REPO}/releases/download/v${VERSION}/${ASSET_NAME}`;

const VENDOR_DIR = path.join(__dirname, "..", "vendor");
fs.mkdirSync(VENDOR_DIR, { recursive: true });

function download(url, dest, redirects = 5) {
  return new Promise((resolve, reject) => {
    https
      .get(url, (response) => {
        if ([301, 302, 307, 308].includes(response.statusCode) && redirects > 0) {
          response.resume();
          return resolve(download(response.headers.location, dest, redirects - 1));
        }
        if (response.statusCode !== 200) {
          response.resume();
          return reject(new Error(`HTTP ${response.statusCode} for ${url}`));
        }
        const file = fs.createWriteStream(dest);
        response.pipe(file);
        file.on("finish", () => file.close(resolve));
        file.on("error", reject);
      })
      .on("error", reject);
  });
}

async function main() {
  if (process.env.ARTUI_SKIP_POSTINSTALL === "1") {
    console.log("ARTUI_SKIP_POSTINSTALL set — skipping binary download.");
    return;
  }
  const tmp = path.join(VENDOR_DIR, ASSET_NAME);
  console.log(`Downloading ${ASSET_URL}`);
  await download(ASSET_URL, tmp);

  if (ARCHIVE_EXT === "tar.gz") {
    if (tar) {
      await tar.x({ file: tmp, cwd: VENDOR_DIR });
    } else {
      execFileSync("tar", ["-xzf", tmp, "-C", VENDOR_DIR], { stdio: "inherit" });
    }
  } else {
    // Use built-in expand on Windows.
    execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-Command",
        `Expand-Archive -Path '${tmp}' -DestinationPath '${VENDOR_DIR}' -Force`,
      ],
      { stdio: "inherit" }
    );
  }

  // Locate the binary anywhere under vendor/ and hoist it to the top level.
  const binName = isWin ? "artui.exe" : "artui";
  const found = walkFind(VENDOR_DIR, binName);
  if (!found) {
    throw new Error(`Could not locate ${binName} after extraction`);
  }
  const target = path.join(VENDOR_DIR, binName);
  if (found !== target) {
    fs.renameSync(found, target);
  }
  if (!isWin) {
    fs.chmodSync(target, 0o755);
  }
  fs.unlinkSync(tmp);
  console.log(`Installed artui ${VERSION} → ${target}`);
}

function walkFind(dir, name) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      const hit = walkFind(full, name);
      if (hit) return hit;
    } else if (entry.name === name) {
      return full;
    }
  }
  return null;
}

main().catch((error) => {
  console.error("artui postinstall failed:", error.message);
  console.error(
    "You can build from source: ARTUI_FROM_SOURCE=1 curl -fsSL " +
      "https://raw.githubusercontent.com/" +
      REPO +
      "/main/scripts/install.sh | sh"
  );
  process.exit(1);
});
