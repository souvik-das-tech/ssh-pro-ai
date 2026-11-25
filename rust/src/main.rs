//! Entry point: `sshpro.exe` (desktop window) or `sshpro.exe --serve` (backend only, prints the URL
//! to open in a browser; for development).
//!
//! The window is a native WebView2 view (wry + tao) showing the same web UI as the Python app,
//! served by the Rust backend on 127.0.0.1 with a per-launch token.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agy;
mod backup;
mod brain;
mod codex;
mod crypto;
mod db;
mod importer;
mod localexec;
mod paths;
mod router;
mod safety;
mod server;
mod shlex;
mod sshpool;
mod util;

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use crate::db::Database;
use crate::paths::{db_path, logs_dir};
use crate::server::Backend;

const TITLE: &str = "SSH Pro AI";

fn setup_logging() -> tracing_appender::non_blocking::WorkerGuard {
    use tracing_subscriber::prelude::*;
    let file = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("sshpro-rust")
        .filename_suffix("log")
        .max_log_files(5)
        .build(logs_dir())
        .expect("log folder");
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = tracing_subscriber::EnvFilter::try_from_env("SSHPRO_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,russh=warn,russh_sftp=warn,hyper=warn"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(writer).with_ansi(false))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();
    guard
}

struct Args {
    serve: bool,
    port: u16,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { serve: false, port: 0 };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--serve" => args.serve = true,
            "--port" => args.port = it.next().and_then(|p| p.parse().ok()).ok_or("--port needs a number")?,
            "-h" | "--help" => {
                return Err("usage: sshpro [--serve] [--port N]\n  --serve  backend only; print the URL".into())
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(args)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().enable_all().thread_name("sshpro-backend").build().expect("tokio runtime")
}

/// Open the database and start the backend; returns it with the UI's URL.
async fn start_backend(port: u16) -> anyhow::Result<(Arc<Backend>, String)> {
    let db = Arc::new(Database::open(&db_path())?);
    let backend = Backend::new(db, None);
    let url = server::serve(backend.clone(), port).await?;
    Ok((backend, url))
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            win::attach_console();
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let _log = setup_logging();
    if args.serve {
        win::attach_console();
        let rt = runtime();
        return rt.block_on(async {
            let (backend, url) = match start_backend(args.port).await {
                Ok(x) => x,
                Err(e) => {
                    eprintln!("Backend failed to start: {e}");
                    return ExitCode::FAILURE;
                }
            };
            println!("{url}");
            let _ = tokio::signal::ctrl_c().await;
            let _ = tokio::time::timeout(Duration::from_secs(10), backend.shutdown()).await;
            ExitCode::SUCCESS
        });
    }
    run_window(args.port)
}

#[cfg(windows)]
fn run_window(port: u16) -> ExitCode {
    use tao::dpi::LogicalSize;
    use tao::event::{Event, StartCause, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::platform::windows::{IconExtWindows, WindowExtWindows};
    use tao::window::{Icon, Theme, WindowBuilder};
    use wry::{NewWindowResponse, PermissionKind, PermissionResponse, WebContext, WebViewBuilder};

    win::set_app_id();
    // One window at a time: a second double-click brings the first to the front.
    let Some(show_signal) = win::single_instance() else {
        return ExitCode::SUCCESS;
    };

    let rt = runtime();
    let (backend, url) = match rt.block_on(start_backend(port)) {
        Ok(x) => x,
        Err(e) => {
            tracing::error!("Backend failed to start: {e:#}");
            rfd::MessageDialog::new()
                .set_title(TITLE)
                .set_level(rfd::MessageLevel::Error)
                .set_description(format!("Backend failed to start:\n{e:#}"))
                .show();
            return ExitCode::FAILURE;
        }
    };
    tracing::info!("SSH Pro AI {} (Rust) on {}", env!("CARGO_PKG_VERSION"), url.split('?').next().unwrap_or(""));

    let event_loop = EventLoopBuilder::<()>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    win::watch_show_signal(show_signal, move || {
        let _ = proxy.send_event(());
    });

    let window = WindowBuilder::new()
        .with_title(TITLE)
        .with_inner_size(LogicalSize::new(1480.0, 920.0))
        .with_min_inner_size(LogicalSize::new(980.0, 620.0))
        .with_theme(Some(Theme::Dark))
        .with_window_icon(Icon::from_resource(1, None).ok())
        .build(&event_loop)
        .expect("window");
    win::dark_title_bar(window.hwnd());

    let origin = url.split("/?").next().unwrap_or("").to_string();
    let mut context = WebContext::new(Some(paths::data_dir().join("webview2")));
    let webview = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(&url)
        .with_background_color((11, 15, 23, 255))
        .with_clipboard(true)
        .with_devtools(cfg!(debug_assertions))
        .with_permission_handler(|kind| match kind {
            // xterm.js paste and "copy answer" use the clipboard (Qt: JavascriptCanPaste)
            PermissionKind::ClipboardRead => PermissionResponse::Allow,
            _ => PermissionResponse::Default,
        })
        .with_download_started_handler(|_url, path| {
            // Downloads (the backup export) only go where the user says: ask where to save.
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "backup.sshpro".into());
            let chosen = rfd::FileDialog::new()
                .set_title("Save backup")
                .set_directory(paths::home_dir().join("Documents"))
                .set_file_name(name)
                .add_filter("SSH Pro AI backup", &["sshpro"])
                .add_filter("All files", &["*"])
                .save_file();
            match chosen {
                Some(p) => {
                    *path = p;
                    true
                }
                None => false,
            }
        })
        .with_new_window_req_handler(|url, _| {
            server::open_in_browser(&url);
            NewWindowResponse::Deny
        })
        .with_navigation_handler(move |nav| {
            if nav.starts_with(&origin) || nav.starts_with("about:") || nav.starts_with("blob:") || nav.starts_with("data:") {
                return true;
            }
            server::open_in_browser(&nav); // links leave the app for the default browser
            false
        })
        .build(&window);
    let webview = match webview {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("WebView2 failed: {e}");
            rfd::MessageDialog::new()
                .set_title(TITLE)
                .set_level(rfd::MessageLevel::Error)
                .set_description(format!(
                    "The Microsoft Edge WebView2 Runtime is needed to show the window:\n{e}\n\n\
                     Install it from https://developer.microsoft.com/microsoft-edge/webview2/"
                ))
                .show();
            return ExitCode::FAILURE;
        }
    };

    let mut rt = Some(rt);
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        let _keep = (&webview, &context);
        match event {
            // tao re-applies its own frame theme while the window opens: set the dark caption after it
            Event::NewEvents(StartCause::Init) | Event::WindowEvent { event: WindowEvent::ThemeChanged(_), .. } => {
                win::dark_title_bar(window.hwnd());
            }
            Event::UserEvent(()) => {
                tracing::info!("Another start: bringing the window to the front");
                window.set_minimized(false);
                window.set_visible(true);
                window.set_focus();
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                window.set_visible(false);
                // close the AI sessions (their processes) and the SSH connections, then leave: a pipe
                // read of a finished child can still hold a runtime thread, so do not wait for it
                if let Some(rt) = rt.take() {
                    rt.block_on(async {
                        let _ = tokio::time::timeout(Duration::from_secs(10), backend.shutdown()).await;
                    });
                    rt.shutdown_timeout(Duration::from_secs(2));
                }
                tracing::info!("Window closed");
                *control_flow = ControlFlow::Exit;
                std::process::exit(0);
            }
            _ => {}
        }
    })
}

#[cfg(not(windows))]
fn run_window(_port: u16) -> ExitCode {
    eprintln!("The desktop window needs Windows; use --serve and open the URL in a browser.");
    ExitCode::FAILURE
}

/// Small Win32 helpers: console for --serve, taskbar identity, single instance, dark title bar.
#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
    use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject, INFINITE};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn attach_console() {
        unsafe { windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS) };
    }

    /// Own taskbar group and icon.
    pub fn set_app_id() {
        let id = wide("SSHProAI.Desktop");
        unsafe { windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(id.as_ptr()) };
    }

    pub struct ShowSignal(HANDLE);
    unsafe impl Send for ShowSignal {}

    /// None if another instance runs (it was asked to come to the front).
    pub fn single_instance() -> Option<ShowSignal> {
        let user = std::env::var("USERNAME").unwrap_or_default();
        let name = wide(&format!("Local\\SSHProAI-Rust-{user}"));
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        if event.is_null() {
            return Some(ShowSignal(event)); // cannot tell: just run
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // this process got the user's click: let the running one take the foreground
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(
                    windows_sys::Win32::UI::WindowsAndMessaging::ASFW_ANY,
                );
                SetEvent(event)
            };
            return None;
        }
        Some(ShowSignal(event))
    }

    pub fn watch_show_signal(signal: ShowSignal, on_show: impl Fn() + Send + 'static) {
        if signal.0.is_null() {
            return;
        }
        std::thread::spawn(move || {
            let signal = signal;
            loop {
                if unsafe { WaitForSingleObject(signal.0, INFINITE) } != 0 {
                    return;
                }
                on_show();
            }
        });
    }

    pub fn dark_title_bar(hwnd: isize) {
        use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
        // Windows 11: DWMWA_USE_IMMERSIVE_DARK_MODE = 20, DWMWA_CAPTION_COLOR = 35
        let on: i32 = 1;
        let color: u32 = 0x0017_0F0B; // COLORREF 0x00BBGGRR -> #0b0f17
        unsafe {
            DwmSetWindowAttribute(hwnd as _, 20, &on as *const _ as _, 4);
            DwmSetWindowAttribute(hwnd as _, 35, &color as *const _ as _, 4);
        }
    }
}

#[cfg(not(windows))]
mod win {
    pub fn attach_console() {}
}
