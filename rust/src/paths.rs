//! Filesystem locations. Runtime data (db, logs, settings) lives in %LOCALAPPDATA%\SSH-Pro-AI, the
//! same folder the Python app uses, so both read the same servers and chat history.

use std::path::PathBuf;

pub const APP_NAME: &str = "SSH-Pro-AI";

/// Override with SSHPRO_DATA_DIR (used by tests).
pub fn data_dir() -> PathBuf {
    let base = match std::env::var_os("SSHPRO_DATA_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            let local = std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home_dir().join("AppData").join("Local"));
            local.join(APP_NAME)
        }
    };
    let _ = std::fs::create_dir_all(&base);
    base
}

pub fn logs_dir() -> PathBuf {
    let path = data_dir().join("logs");
    let _ = std::fs::create_dir_all(&path);
    path
}

pub fn db_path() -> PathBuf {
    data_dir().join("sshpro.sqlite3")
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A Termix `ssh.sqlite` export next to the app (the exe's folder, or the repo root above
/// `rust/target/...` during development) is imported on the first start.
pub fn termix_db() -> PathBuf {
    if let Some(path) = std::env::var_os("SSHPRO_TERMIX_DB") {
        return PathBuf::from(path);
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let mut dir = exe.parent().map(PathBuf::from);
    while let Some(d) = dir {
        let candidate = d.join("ssh.sqlite");
        if candidate.is_file() {
            return candidate;
        }
        dir = d.parent().map(PathBuf::from);
        if dir.as_ref().is_some_and(|p| p.join(".git").exists()) {
            return dir.unwrap().join("ssh.sqlite");
        }
    }
    exe.parent().map(|p| p.join("ssh.sqlite")).unwrap_or_else(|| PathBuf::from("ssh.sqlite"))
}

/// Which `name.exe` on PATH, like `shutil.which`.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .map(|e| e.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        if cfg!(windows) {
            if std::path::Path::new(name).extension().is_some() && dir.join(name).is_file() {
                return Some(dir.join(name));
            }
            for ext in &exts {
                let candidate = dir.join(format!("{name}{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        } else if dir.join(name).is_file() {
            return Some(dir.join(name));
        }
    }
    None
}
