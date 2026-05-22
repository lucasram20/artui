// Ink-driven artui installer UI. Renders a logo, log feed, spinner, and
// progress bar using Claude Code's UI framework (ink + react).
//
// Behaviour:
// - On a TTY, mounts the React tree and updates state as the install
//   progresses through steps.
// - On non-TTY/CI environments, falls back to plain console logging via
//   the `--plain` flag (set automatically when stdout isn't a TTY).
import React, { useEffect, useReducer } from "react";
import { Box, Text, render } from "ink";

import { runInstall, type InstallEvent, type InstallStep } from "./install.js";

interface UiState {
  steps: InstallStep[];
  progress: { ratio: number; suffix: string } | null;
  error: string | null;
  done: boolean;
  version: string;
}

type Action =
  | { type: "step:add"; step: InstallStep }
  | { type: "step:update"; index: number; step: InstallStep }
  | { type: "progress"; ratio: number; suffix: string }
  | { type: "progress:clear" }
  | { type: "error"; message: string }
  | { type: "done" }
  | { type: "version"; version: string };

const initialState: UiState = {
  steps: [],
  progress: null,
  error: null,
  done: false,
  version: "",
};

function reducer(state: UiState, action: Action): UiState {
  switch (action.type) {
    case "step:add":
      return { ...state, steps: [...state.steps, action.step] };
    case "step:update": {
      const next = state.steps.slice();
      next[action.index] = action.step;
      return { ...state, steps: next };
    }
    case "progress":
      return {
        ...state,
        progress: { ratio: action.ratio, suffix: action.suffix },
      };
    case "progress:clear":
      return { ...state, progress: null };
    case "error":
      return { ...state, error: action.message };
    case "done":
      return { ...state, done: true };
    case "version":
      return { ...state, version: action.version };
    default:
      return state;
  }
}

const LOGO_LINES = [
  "  █████╗ ██████╗ ████████╗██╗   ██╗██╗",
  " ██╔══██╗██╔══██╗╚══██╔══╝██║   ██║██║",
  " ███████║██████╔╝   ██║   ██║   ██║██║",
  " ██╔══██║██╔══██╗   ██║   ██║   ██║██║",
  " ██║  ██║██║  ██║   ██║   ╚██████╔╝██║",
  " ╚═╝  ╚═╝╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚═╝",
];

function Logo({ version }: { version: string }): JSX.Element {
  return (
    <Box flexDirection="column" marginBottom={1}>
      {LOGO_LINES.map((line, i) => (
        <Text key={i} color="cyan" bold>
          {line}
        </Text>
      ))}
      <Text dimColor>  interactive coding-agent CLI · v{version || "?"}</Text>
    </Box>
  );
}

const SPINNER_FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

function useSpinnerFrame(active: boolean): string {
  const [frame, tick] = useReducer((n: number) => (n + 1) % SPINNER_FRAMES.length, 0);
  useEffect(() => {
    if (!active) return;
    const id = setInterval(tick, 80);
    return () => clearInterval(id);
  }, [active]);
  return SPINNER_FRAMES[frame] ?? SPINNER_FRAMES[0]!;
}

function StepRow({ step, isCurrent }: { step: InstallStep; isCurrent: boolean }): JSX.Element {
  const frame = useSpinnerFrame(isCurrent && step.status === "running");
  if (step.status === "running") {
    return (
      <Text>
        <Text color="cyan">{frame}</Text> {step.label}
      </Text>
    );
  }
  if (step.status === "ok") {
    return (
      <Text>
        <Text color="green">✔</Text> {step.label}
      </Text>
    );
  }
  return (
    <Text>
      <Text color="red">✖</Text> {step.label}
    </Text>
  );
}

function ProgressBar({ ratio, suffix }: { ratio: number; suffix: string }): JSX.Element {
  const width = 28;
  const filled = Math.max(0, Math.min(width, Math.round(ratio * width)));
  const bar = "█".repeat(filled) + "░".repeat(width - filled);
  const pct = `${Math.round(ratio * 100)
    .toString()
    .padStart(3)}%`;
  return (
    <Box>
      <Text color="cyan">{bar}</Text>
      <Text> {pct}</Text>
      {suffix ? <Text dimColor> {suffix}</Text> : null}
    </Box>
  );
}

function Installer({ version }: { version: string }): JSX.Element {
  const [state, dispatch] = useReducer(reducer, { ...initialState, version });

  useEffect(() => {
    let mounted = true;
    runInstall(version, (event: InstallEvent) => {
      if (!mounted) return;
      switch (event.kind) {
        case "version":
          dispatch({ type: "version", version: event.version });
          break;
        case "step:start":
          dispatch({ type: "step:add", step: { label: event.label, status: "running" } });
          break;
        case "step:end":
          dispatch({
            type: "step:update",
            index: event.index,
            step: { label: event.label, status: event.ok ? "ok" : "fail" },
          });
          break;
        case "progress":
          dispatch({ type: "progress", ratio: event.ratio, suffix: event.suffix ?? "" });
          break;
        case "progress:clear":
          dispatch({ type: "progress:clear" });
          break;
        case "error":
          dispatch({ type: "error", message: event.message });
          break;
        case "done":
          dispatch({ type: "done" });
          break;
      }
    })
      .catch((err: unknown) => {
        if (!mounted) return;
        dispatch({
          type: "error",
          message: err instanceof Error ? err.message : String(err),
        });
      })
      .finally(() => {
        // Allow Ink to flush the final frame before exiting.
        setTimeout(() => process.exit(state.error ? 1 : 0), 60);
      });

    return () => {
      mounted = false;
    };
  }, [version]);

  const lastIndex = state.steps.length - 1;

  return (
    <Box flexDirection="column">
      <Logo version={state.version} />
      {state.steps.map((step, i) => (
        <StepRow key={i} step={step} isCurrent={i === lastIndex && !state.done} />
      ))}
      {state.progress ? (
        <Box marginTop={1}>
          <ProgressBar ratio={state.progress.ratio} suffix={state.progress.suffix} />
        </Box>
      ) : null}
      {state.error ? (
        <Box marginTop={1}>
          <Text color="red">✖ {state.error}</Text>
        </Box>
      ) : null}
      {state.done && !state.error ? (
        <Box marginTop={1}>
          <Text color="green">✔ artui v{state.version} installed.</Text>
        </Box>
      ) : null}
    </Box>
  );
}

export function startUi(version: string): void {
  render(<Installer version={version} />);
}
