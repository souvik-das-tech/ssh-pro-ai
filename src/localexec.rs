//! Commands on this Windows PC for the AI: find a file, build a project, pack it before an upload.
//!
//! Runs Windows PowerShell non-interactively without a console window. Results have the same shape
//! as `SshManager::run` so the AI loop treats this PC like one more server.

use std::process::Stdio;
use std::time::{Duration, Instant};

use base64::Engine;
use tokio::io::AsyncReadExt;

use crate::util::{command, expand, ExecResult, KillGuard};

/// UTF-8 output, no progress bars; the exit code reflects the last statement (cmdlet or program).
const PRELUDE: &str = "$ProgressPreference = 'SilentlyContinue'; [Console]::OutputEncoding = [Text.Encoding]::UTF8\n";
const EPILOGUE: &str = "\nif (-not $?) { if ($LASTEXITCODE) { exit $LASTEXITCODE } else { exit 1 } }\n";

fn text(data: &[u8]) -> String {
    String::from_utf8_lossy(data).replace("\r\n", "\n")
}

/// Run one PowerShell command. Never fails for command failures: they come back in the result.
pub async fn run_local(cmd: &str, cwd: Option<&str>, timeout: f64) -> ExecResult {
    let started = Instant::now();
    let cwd = cwd.map(expand);
    if let Some(dir) = &cwd {
        if !std::path::Path::new(dir).is_dir() {
            return ExecResult::error(started, format!("Folder not found on this PC: {dir}"));
        }
    }
    let script: Vec<u8> = format!("{PRELUDE}{cmd}{EPILOGUE}")
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(script);
    let mut proc = command("powershell.exe");
    proc.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = &cwd {
        proc.current_dir(dir);
    }
    let mut child = match proc.spawn() {
        Ok(c) => c,
        Err(e) => return ExecResult::error(started, format!("Could not start PowerShell: {e}")),
    };
    let mut guard = KillGuard(child.id());
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let work = async {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let (_, _, status) = tokio::join!(stdout.read_to_end(&mut out), stderr.read_to_end(&mut err), child.wait());
        (out, err, status)
    };
    match tokio::time::timeout(Duration::from_secs_f64(timeout.max(1.0)), work).await {
        Ok((out, err, status)) => {
            guard.disarm();
            let code = status.ok().and_then(|s| s.code()).map(|c| c as i64);
            ExecResult::new(started, code, &text(&out), &text(&err), None)
        }
        Err(_) => {
            drop(guard); // kills the process tree
            ExecResult::error(started, format!("Timed out after {timeout:.0}s"))
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runs_powershell() {
        let res = run_local("Write-Output 'hi'; [Console]::Error.WriteLine('oops')", None, 60.0).await;
        assert_eq!(res.exit, Some(0), "{res:?}");
        assert_eq!(res.stdout.trim(), "hi");
        assert_eq!(res.stderr.trim(), "oops");
        let res = run_local("exit 3", None, 60.0).await;
        assert_eq!(res.exit, Some(3));
        let res = run_local("dir", Some("Z:\\definitely\\missing"), 60.0).await;
        assert!(res.error.unwrap().starts_with("Folder not found"));
    }
}
