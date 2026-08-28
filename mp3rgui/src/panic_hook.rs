//! Panic hook for the windowed build (issue #297).
//!
//! On Windows the GUI has no console (`windows_subsystem = "windows"`), so a
//! panic anywhere — winit/wgpu during startup, the render loop, our own
//! `update()` — kills the process with nothing the user can see. The hook
//! makes that failure visible and reportable: log first (stderr plus a
//! `panic.log` in the eframe data dir), dialog last, and only on the main
//! thread. A modal for a worker-thread panic would turn a survivable glitch
//! into a scary popup, and a dialog from a panicking event loop is
//! best-effort anyway — if the platform state is too broken to show it, we
//! are no worse off than today.

use std::io::Write as _;
use std::panic::PanicHookInfo;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// Where panics are recorded: next to eframe's own `app.ron`, so a portable
/// install leaves no new directory behind. `None` if eframe has no data dir.
fn log_path() -> Option<PathBuf> {
    eframe::storage_dir("mp3rgain").map(|dir| dir.join("panic.log"))
}

/// Appends normally; a log past this size is truncated first so repeated
/// crashes cannot grow the file without bound.
const MAX_LOG_BYTES: u64 = 64 * 1024;

pub fn install() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // The default hook prints message + location (and a backtrace with
        // RUST_BACKTRACE) to stderr, for anyone running from a terminal.
        default_hook(info);

        // A panic inside this hook must not recurse into it.
        static IN_HOOK: AtomicBool = AtomicBool::new(false);
        if IN_HOOK.swap(true, Ordering::SeqCst) {
            return;
        }

        let message = describe(info);
        let logged = write_log(&message);

        if std::thread::current().name() == Some("main") {
            let where_to = match &logged {
                Some(path) => format!("This was written to:\n{}\n\n", path.display()),
                None => String::new(),
            };
            let text = format!(
                "{message}\n\n{where_to}Please report it at https://github.com/M-Igashi/mp3rgain/issues"
            );
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                rfd::MessageDialog::new()
                    .set_title("mp3rgain crashed")
                    .set_description(&text)
                    .set_level(rfd::MessageLevel::Error)
                    .show();
            }));
        }

        IN_HOOK.store(false, Ordering::SeqCst);
    }));
}

fn describe(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    let msg = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic payload>");
    let location = info
        .location()
        .map(|l| l.to_string())
        .unwrap_or_else(|| "<unknown location>".to_string());
    let thread = std::thread::current();
    format!(
        "mp3rgui {} panicked on thread '{}' at {location}:\n{msg}",
        env!("CARGO_PKG_VERSION"),
        thread.name().unwrap_or("<unnamed>"),
    )
}

/// Best-effort: any I/O failure just means no log file, never a panic.
fn write_log(message: &str) -> Option<PathBuf> {
    let path = log_path()?;
    std::fs::create_dir_all(path.parent()?).ok()?;
    let truncate = std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(!truncate)
        .truncate(truncate)
        .open(&path)
        .ok()?;
    let unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    writeln!(file, "[unix {unix_secs}] {message}\n").ok()?;
    Some(path)
}
