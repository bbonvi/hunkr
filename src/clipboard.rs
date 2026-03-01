use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipboardBackend {
    WlCopy,
    Xclip,
    Xsel,
    Pbcopy,
    ClipExe,
    Clip,
    Tmux,
}

impl ClipboardBackend {
    fn label(self) -> &'static str {
        match self {
            Self::WlCopy => "wl-copy",
            Self::Xclip => "xclip",
            Self::Xsel => "xsel",
            Self::Pbcopy => "pbcopy",
            Self::ClipExe => "clip.exe",
            Self::Clip => "clip",
            Self::Tmux => "tmux",
        }
    }

    fn command(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::WlCopy => ("wl-copy", &[]),
            Self::Xclip => ("xclip", &["-selection", "clipboard", "-in"]),
            Self::Xsel => ("xsel", &["--clipboard", "--input"]),
            Self::Pbcopy => ("pbcopy", &[]),
            Self::ClipExe => ("clip.exe", &[]),
            Self::Clip => ("clip", &[]),
            Self::Tmux => ("tmux", &["load-buffer", "-w", "-"]),
        }
    }
}

/// Copies `text` to the system clipboard by trying multiple backends in order.
///
/// Returns the backend label that succeeded (for user-facing status messages).
pub fn copy_to_clipboard_with_fallbacks(text: &str) -> anyhow::Result<&'static str> {
    let backends = candidate_backends_from_env(|name| std::env::var_os(name).is_some());
    let mut failures = Vec::new();

    for backend in backends {
        match run_backend(backend, text) {
            Ok(()) => return Ok(backend.label()),
            Err(err) => failures.push(format!("{}: {err:#}", backend.label())),
        }
    }

    if failures.is_empty() {
        bail!("no clipboard backend candidates found");
    }
    bail!("all clipboard backends failed ({})", failures.join("; "));
}

fn run_backend(backend: ClipboardBackend, text: &str) -> anyhow::Result<()> {
    let (program, args) = backend.command();
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn `{program}`"))?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .context("clipboard backend stdin unavailable")?;
        stdin
            .write_all(text.as_bytes())
            .with_context(|| format!("failed to write clipboard payload to `{program}`"))?;
    }

    let output = child
        .wait_with_output()
        .with_context(|| format!("failed while waiting for `{program}`"))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if stderr.is_empty() {
        bail!("process exited with status {}", output.status);
    }
    bail!("process exited with status {}: {}", output.status, stderr);
}

fn candidate_backends_from_env<F>(has_env: F) -> Vec<ClipboardBackend>
where
    F: Fn(&str) -> bool,
{
    let mut backends = Vec::new();
    if has_env("WAYLAND_DISPLAY") {
        backends.push(ClipboardBackend::WlCopy);
    }
    if has_env("DISPLAY") {
        backends.push(ClipboardBackend::Xclip);
        backends.push(ClipboardBackend::Xsel);
    }

    backends.push(ClipboardBackend::Pbcopy);
    backends.push(ClipboardBackend::ClipExe);
    backends.push(ClipboardBackend::Clip);
    if has_env("TMUX") {
        backends.push(ClipboardBackend::Tmux);
    }
    backends
}
