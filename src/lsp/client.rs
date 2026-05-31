//! [`LspClient`] — one client per `(server_id, workspace_root)` pair.
//!
//! Implementation note: this module spawns the language server as a child
//! process and drives it through [`async_lsp::MainLoop`]. The
//! [`tower::ServiceBuilder`] middleware chain handles concurrency capping,
//! lifecycle, and the `client-monitor` watchdog that catches a server
//! crashing without a clean shutdown.
//!
//! The public surface is intentionally narrow — Phase N1 only needs
//! [`Self::definition`], [`Self::hover`], and lifecycle primitives. Phases
//! N2/N3 extend this with references / diagnostics / didChange.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use async_lsp::concurrency::ConcurrencyLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::tracing::TracingLayer;
use async_lsp::ServerSocket;
use lsp_types::notification::{Exit, Initialized, LogMessage, PublishDiagnostics, ShowMessage};
use lsp_types::request::{GotoDefinition, HoverRequest, Initialize, Shutdown};
use lsp_types::{
    ClientCapabilities, ClientInfo, Diagnostic, GotoDefinitionParams, GotoDefinitionResponse,
    Hover, HoverClientCapabilities, HoverParams, InitializeParams, InitializedParams, MarkupKind,
    PartialResultParams, Position, PublishDiagnosticsParams, ServerCapabilities,
    TextDocumentClientCapabilities, TextDocumentIdentifier, TextDocumentPositionParams, Url,
    WindowClientCapabilities, WorkDoneProgressParams, WorkspaceFolder,
};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};
use tower::ServiceBuilder;
use tracing::{debug, warn};

use crate::app::AppEvent;

use super::types::ServerSpec;

/// Default time budget for `initialize`. Chosen to be longer than rust-analyzer's
/// cold-start handshake (~3s on a fresh machine) but short enough to fail
/// loud when a binary hangs.
const DEFAULT_INITIALIZE_TIMEOUT: Duration = Duration::from_secs(15);

/// Default time budget for a single LSP request. Long enough that
/// rust-analyzer's first `definition` after indexing finishes returns
/// successfully, short enough that an unresponsive server doesn't wedge
/// the agent loop.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Stable identifier for an LSP language code. Maps from a path's extension
/// to the LSP `languageId` string the server expects. Currently unused at
/// the request layer (Phase N1 only does definition/hover, which the
/// server resolves from the file uri); reserved for Phase N3's didOpen
/// payload.
#[allow(dead_code)]
fn language_id_for_extension(extension: &str) -> &'static str {
    match extension
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "rust",
        "go" => "go",
        "py" | "pyi" => "python",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "c" => "c",
        "h" => "c",
        "cc" | "cpp" | "cxx" | "c++" => "cpp",
        "hpp" | "hxx" | "h++" | "hh" => "cpp",
        "lua" => "lua",
        "rb" => "ruby",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "scala" => "scala",
        "swift" => "swift",
        "zig" => "zig",
        "hs" => "haskell",
        "ml" | "mli" => "ocaml",
        "elm" => "elm",
        "dart" => "dart",
        "ex" | "exs" => "elixir",
        "erl" | "hrl" => "erlang",
        "clj" | "cljs" | "cljc" => "clojure",
        "lisp" | "el" => "lisp",
        "rkt" => "racket",
        "nim" | "nims" => "nim",
        "cr" => "crystal",
        "json" | "jsonc" => "json",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "md" | "markdown" => "markdown",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" | "sass" => "scss",
        "sh" | "bash" => "shellscript",
        "tf" | "tfvars" => "terraform",
        "dockerfile" => "dockerfile",
        "proto" => "proto",
        other => leak_str(other),
    }
}

/// Leak a small string for the rare extension we don't have a static
/// mapping for. Unbounded leaking would be a footgun in a long-lived
/// process, but the set of extensions per session is finite (and small)
/// so the cost is bounded by the workspace's actual file types.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_owned().into_boxed_str())
}

/// State the client cares about per open file. Phase N1 doesn't actually
/// open files (no didOpen / didChange), but the field is here so Phase N3
/// can extend without an API churn.
#[derive(Debug, Default)]
struct ClientState {
    diagnostics: HashMap<PathBuf, Vec<Diagnostic>>,
    capabilities: Option<ServerCapabilities>,
}

pub struct LspClient {
    server_id: String,
    root: PathBuf,
    socket: Mutex<ServerSocket>,
    state: Arc<Mutex<ClientState>>,
    /// Background `MainLoop` task handle. Aborted on drop.
    main_loop: Option<tokio::task::JoinHandle<()>>,
    /// Child process. Killed via `kill_on_drop(true)` if the graceful
    /// shutdown sequence fails.
    child: Option<Child>,
    /// Tracks whether `Drop` should attempt the LSP shutdown handshake.
    /// Cleared once `shutdown` runs successfully.
    needs_shutdown: bool,
}

impl std::fmt::Debug for LspClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspClient")
            .field("server_id", &self.server_id)
            .field("root", &self.root)
            .field("needs_shutdown", &self.needs_shutdown)
            .finish_non_exhaustive()
    }
}

impl LspClient {
    /// Spawn a server matching `spec` rooted at `root`.
    ///
    /// Errors if the executable is not on `$PATH` or the LSP `initialize`
    /// handshake doesn't complete within [`DEFAULT_INITIALIZE_TIMEOUT`].
    pub async fn spawn(
        server_id: &str,
        spec: &ServerSpec,
        root: &Path,
        events: mpsc::Sender<AppEvent>,
    ) -> Result<Self> {
        if which::which(&spec.command).is_err() {
            bail!(
                "language-server executable `{}` not found on $PATH; install it to enable LSP for `{}` files",
                spec.command,
                spec.file_types.first().map(String::as_str).unwrap_or("?")
            );
        }

        let mut command = Command::new(&spec.command);
        command
            .args(&spec.args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (k, v) in &spec.env {
            command.env(k, v);
        }

        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn `{}`", spec.command))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("child stdin pipe missing"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("child stdout pipe missing"))?;
        // Drain stderr so the buffer doesn't fill and stall the child.
        if let Some(stderr) = child.stderr.take() {
            spawn_stderr_drain(server_id.to_owned(), stderr);
        }

        let state = Arc::new(Mutex::new(ClientState::default()));
        let server_id_owned = server_id.to_owned();
        let events_for_router = events.clone();
        let state_for_router = Arc::clone(&state);

        let (mainloop, server) = async_lsp::MainLoop::new_client(move |_server| {
            let mut router: Router<()> = Router::new(());
            let events = events_for_router.clone();
            let state = Arc::clone(&state_for_router);
            let id_for_diag = server_id_owned.clone();
            router
                .notification::<PublishDiagnostics>(move |_, params: PublishDiagnosticsParams| {
                    let events = events.clone();
                    let state = Arc::clone(&state);
                    let id = id_for_diag.clone();
                    tokio::spawn(async move {
                        let path = params.uri.to_file_path().ok();
                        let count = params.diagnostics.len();
                        if let Some(path) = path.clone() {
                            let mut state = state.lock().await;
                            state.diagnostics.insert(path, params.diagnostics);
                        }
                        if let Some(path) = path {
                            let _ = events
                                .send(AppEvent::LspDiagnostics {
                                    server_id: id.clone(),
                                    path,
                                    count,
                                })
                                .await;
                        }
                    });
                    std::ops::ControlFlow::Continue(())
                })
                .notification::<ShowMessage>(|_, params| {
                    debug!(target: "lsp", "showMessage: {:?}", params.message);
                    std::ops::ControlFlow::Continue(())
                })
                .notification::<LogMessage>(|_, params| {
                    debug!(target: "lsp", "logMessage: {:?}", params.message);
                    std::ops::ControlFlow::Continue(())
                });

            ServiceBuilder::new()
                .layer(TracingLayer::default())
                .layer(CatchUnwindLayer::default())
                .layer(ConcurrencyLayer::default())
                .service(router)
        });

        let stdin_compat = tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(stdin);
        let stdout_compat = tokio_util::compat::TokioAsyncReadCompatExt::compat(stdout);
        let main_loop_handle = tokio::spawn(async move {
            if let Err(error) = mainloop.run_buffered(stdout_compat, stdin_compat).await {
                warn!(target: "lsp", "MainLoop ended: {error}");
            }
        });

        let mut client = Self {
            server_id: server_id.to_owned(),
            root: root.to_path_buf(),
            socket: Mutex::new(server.clone()),
            state,
            main_loop: Some(main_loop_handle),
            child: Some(child),
            needs_shutdown: false,
        };

        match tokio::time::timeout(
            DEFAULT_INITIALIZE_TIMEOUT,
            client.initialize_handshake(spec, root, server),
        )
        .await
        {
            Ok(Ok(())) => {
                client.needs_shutdown = true;
                Ok(client)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(anyhow!(
                "language server `{}` did not complete initialize within {:?}",
                spec.command,
                DEFAULT_INITIALIZE_TIMEOUT
            )),
        }
    }

    async fn initialize_handshake(
        &self,
        spec: &ServerSpec,
        root: &Path,
        server: ServerSocket,
    ) -> Result<()> {
        let root_uri = Url::from_file_path(root).map_err(|_| {
            anyhow!(
                "workspace root is not a valid file:// URL: {}",
                root.display()
            )
        })?;
        let workspace_folder = WorkspaceFolder {
            uri: root_uri.clone(),
            name: root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "workspace".to_owned()),
        };

        let init_options = spec.init_options_value();
        #[allow(deprecated)]
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            root_path: None,
            root_uri: Some(root_uri),
            initialization_options: init_options,
            capabilities: client_capabilities(),
            trace: None,
            workspace_folders: Some(vec![workspace_folder]),
            client_info: Some(ClientInfo {
                name: "artui".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
            locale: None,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };

        let result = server
            .request::<Initialize>(params)
            .await
            .with_context(|| format!("`initialize` failed for {}", self.server_id))?;
        {
            let mut state = self.state.lock().await;
            state.capabilities = Some(result.capabilities);
        }
        server
            .notify::<Initialized>(InitializedParams {})
            .with_context(|| format!("`initialized` failed for {}", self.server_id))?;
        Ok(())
    }

    pub fn server_id(&self) -> &str {
        &self.server_id
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Snapshot of the cached server capabilities (populated by
    /// `initialize`). Returns `None` if initialize hasn't completed.
    pub async fn capabilities(&self) -> Option<ServerCapabilities> {
        self.state.lock().await.capabilities.clone()
    }

    /// Number of diagnostics tracked for `path`. 0 if path is unknown.
    pub async fn diagnostics_count(&self, path: &Path) -> usize {
        self.state
            .lock()
            .await
            .diagnostics
            .get(path)
            .map(Vec::len)
            .unwrap_or(0)
    }

    /// `textDocument/definition` — return type lets the renderer produce a
    /// list of [`super::types::LocationView`].
    pub async fn definition(
        &self,
        path: &Path,
        line: u32,
        column: u32,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let caps = self.capabilities().await;
        if caps.and_then(|c| c.definition_provider).is_none() {
            return Err(anyhow!(
                "server `{}` does not advertise definition capability",
                self.server_id
            ));
        }
        let uri = path_to_url(path)?;
        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: lsp_position(line, column),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let socket = self.socket.lock().await.clone();
        let response = tokio::time::timeout(
            DEFAULT_REQUEST_TIMEOUT,
            socket.request::<GotoDefinition>(params),
        )
        .await
        .map_err(|_| anyhow!("definition request timed out"))?
        .with_context(|| format!("definition request to {} failed", self.server_id))?;
        Ok(response)
    }

    /// `textDocument/hover` — caller renders into a [`super::types::HoverView`].
    pub async fn hover(&self, path: &Path, line: u32, column: u32) -> Result<Option<Hover>> {
        let caps = self.capabilities().await;
        if caps.and_then(|c| c.hover_provider).is_none() {
            return Err(anyhow!(
                "server `{}` does not advertise hover capability",
                self.server_id
            ));
        }
        let uri = path_to_url(path)?;
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: lsp_position(line, column),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let socket = self.socket.lock().await.clone();
        let response = tokio::time::timeout(
            DEFAULT_REQUEST_TIMEOUT,
            socket.request::<HoverRequest>(params),
        )
        .await
        .map_err(|_| anyhow!("hover request timed out"))?
        .with_context(|| format!("hover request to {} failed", self.server_id))?;
        Ok(response)
    }

    /// Send `shutdown` + `exit`, then wait briefly for the child to exit.
    /// Falls back to `kill_on_drop` semantics on timeout.
    pub async fn shutdown(&mut self) {
        if !self.needs_shutdown {
            return;
        }
        self.needs_shutdown = false;
        let socket = self.socket.lock().await.clone();
        let _ = tokio::time::timeout(Duration::from_secs(2), socket.request::<Shutdown>(())).await;
        let _ = socket.notify::<Exit>(());
        if let Some(handle) = self.main_loop.take() {
            handle.abort();
        }
        if let Some(mut child) = self.child.take() {
            // Best effort: wait, then drop (kill_on_drop fires on drop).
            let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        }
    }
}

fn lsp_position(one_based_line: u32, one_based_column: u32) -> Position {
    Position {
        line: one_based_line.saturating_sub(1),
        character: one_based_column.saturating_sub(1),
    }
}

fn path_to_url(path: &Path) -> Result<Url> {
    Url::from_file_path(path)
        .map_err(|_| anyhow!("invalid path for file:// URL: {}", path.display()))
}

fn client_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        text_document: Some(TextDocumentClientCapabilities {
            hover: Some(HoverClientCapabilities {
                content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                ..Default::default()
            }),
            ..Default::default()
        }),
        window: Some(WindowClientCapabilities::default()),
        ..Default::default()
    }
}

fn spawn_stderr_drain(server_id: String, stderr: tokio::process::ChildStderr) {
    use tokio::io::AsyncBufReadExt;
    use tokio::io::BufReader;
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            debug!(target: "lsp", server = %server_id, "[stderr] {line}");
        }
    });
}

impl Drop for LspClient {
    fn drop(&mut self) {
        if self.needs_shutdown {
            // Best-effort: spawn a detached task to send shutdown/exit. The
            // child has `kill_on_drop(true)` so even if the shutdown task
            // races and loses, the OS will reap the process when the
            // `Child` is dropped below.
            let mut socket = self.socket.try_lock().ok().map(|guard| guard.clone());
            if let Some(socket) = socket.take() {
                let socket = socket;
                tokio::spawn(async move {
                    let _ = tokio::time::timeout(
                        Duration::from_millis(500),
                        socket.request::<Shutdown>(()),
                    )
                    .await;
                    let _ = socket.notify::<Exit>(());
                });
            }
            if let Some(handle) = self.main_loop.take() {
                handle.abort();
            }
            // child drops here; kill_on_drop ensures no orphan.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_based_position_to_zero_based() {
        let pos = lsp_position(12, 8);
        assert_eq!(pos.line, 11);
        assert_eq!(pos.character, 7);
    }

    #[test]
    fn position_handles_zero_input() {
        let pos = lsp_position(0, 0);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn language_id_known_extensions() {
        assert_eq!(language_id_for_extension("rs"), "rust");
        assert_eq!(language_id_for_extension(".rs"), "rust");
        assert_eq!(language_id_for_extension("RS"), "rust");
        assert_eq!(language_id_for_extension("tsx"), "typescriptreact");
        assert_eq!(language_id_for_extension("py"), "python");
        assert_eq!(language_id_for_extension("hpp"), "cpp");
    }

    #[tokio::test]
    async fn spawn_returns_clean_error_when_executable_missing() {
        let spec = ServerSpec {
            command: "this-server-definitely-does-not-exist-zzz".to_owned(),
            args: vec![],
            file_types: vec!["xx".to_owned()],
            root_markers: vec![],
            init_options_json: None,
            init_options: None,
            env: Default::default(),
        };
        let dir = tempfile::TempDir::new().unwrap();
        let (tx, _rx) = mpsc::channel(1);
        let result = LspClient::spawn("missing", &spec, dir.path(), tx).await;
        assert!(result.is_err());
        let msg = format!("{:?}", result.unwrap_err());
        assert!(
            msg.contains("not found"),
            "expected friendly missing-binary error, got: {msg}"
        );
    }
}
