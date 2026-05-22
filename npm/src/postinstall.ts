// Entry point invoked by `npm install`. Picks the Ink UI on TTY,
// falls back to plain console output in CI / NO_COLOR / non-TTY pipes.
import { readVersionFromPackage, runPlain } from "./install.js";

if (process.env.ARTUI_SKIP_POSTINSTALL === "1") {
  process.stdout.write("ARTUI_SKIP_POSTINSTALL set — skipping binary download.\n");
  process.exit(0);
}

// `npm install -g artui --abort` (or any of the env opt-outs) lets users
// install the wrapper without downloading the native binary.
const ABORT_FLAGS = new Set(["--abort", "--no-install", "--skip-binary"]);
if (process.argv.slice(2).some((arg) => ABORT_FLAGS.has(arg))) {
  process.stdout.write(
    "Skipped artui binary download (--abort).\n" +
      "Run `ARTUI_SKIP_POSTINSTALL=0 npm rebuild artui` later to fetch it.\n",
  );
  process.exit(0);
}
if (process.env.ARTUI_INSTALL_YES === "0") {
  process.stdout.write(
    "ARTUI_INSTALL_YES=0 — install aborted before downloading the artui binary.\n",
  );
  process.exit(0);
}

const version = readVersionFromPackage();
const interactive = Boolean(process.stdout.isTTY) && !process.env.CI && !process.env.NO_COLOR;

if (interactive) {
  // Lazy-load the Ink renderer so non-TTY installs don't pull React deps.
  const { startUi } = await import("./ui.js");
  startUi(version);
} else {
  runPlain(version).catch((err: unknown) => {
    process.stderr.write(`✖ ${(err as Error).message}\n`);
    process.exit(1);
  });
}
