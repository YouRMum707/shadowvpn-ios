//! Logging for the ShadowVPN data plane.
//!
//! Every `log::{info,warn,error,debug}!` in this crate — and every
//! `svpn_core_log` call from the ObjC NetworkExtension host — goes to Apple's
//! unified log (os_log) via one global logger, viewable with
//! `log stream --predicate 'subsystem == "com.tangzixiang.shadowvpn.PacketTunnel"'`
//! or `idevicesyslog`.
//!
//! In addition, every **info-or-higher** record is mirrored to a small,
//! line-based file in the App Group container (`logs/svpn-tunnel.log`, the path
//! the host hands us via [`set_log_file`]). The app's Log view tails that file —
//! the NetworkExtension's own `OSLogStore` is not readable from the app. Debug/
//! trace records (e.g. the 2 Hz traffic pump) stay in os_log only so the file
//! stays small; the file is rotated to `.1` once it crosses [`MAX_LOG_BYTES`].

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, Once, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use log::{Level, LevelFilter, Log, Metadata, Record};
use oslog::OsLog;

static INIT: Once = Once::new();

/// os_log subsystem — the PacketTunnel extension's bundle id, matching the
/// `os_log` subsystem the ObjC `SV*` classes use, so engine + NE lifecycle
/// lines interleave on one timeline.
const OSLOG_SUBSYSTEM: &str = "com.tangzixiang.shadowvpn.PacketTunnel";

/// Rotate the mirrored log file to `<name>.1` once it grows past this.
const MAX_LOG_BYTES: u64 = 512 * 1024;

/// Append handle for the shared log file, installed by [`set_log_file`]. `None`
/// until the host sets the home dir.
fn log_file() -> &'static Mutex<Option<File>> {
    static F: OnceLock<Mutex<Option<File>>> = OnceLock::new();
    F.get_or_init(|| Mutex::new(None))
}

/// Global logger: fans every record out to os_log, and info+ to the shared file.
struct Logger {
    os: OsLog,
}

impl Log for Logger {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= Level::Debug
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let msg = format!("{}", record.args());
        self.os.with_level(record.level().into(), &msg);

        // Mirror info+ to the file the app's Log view tails.
        if record.level() <= Level::Info {
            if let Ok(mut guard) = log_file().lock() {
                if let Some(f) = guard.as_mut() {
                    let secs = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    // "HH:MM:SS LEVEL message" (UTC). The app sniffs the LEVEL
                    // token for row tinting; it doesn't require a strict format.
                    let _ = writeln!(
                        f,
                        "{:02}:{:02}:{:02} {:<5} {}",
                        (secs / 3600) % 24,
                        (secs / 60) % 60,
                        secs % 60,
                        record.level(),
                        msg,
                    );
                }
            }
        }
    }

    fn flush(&self) {}
}

/// Initialize logging. Idempotent — safe to call from every `svpn_core_init`,
/// which the NE may invoke more than once across restarts.
pub fn init_os_logger() {
    INIT.call_once(|| {
        let logger = Logger {
            os: OsLog::new(OSLOG_SUBSYSTEM, "core"),
        };
        if let Err(e) = log::set_boxed_logger(Box::new(logger)) {
            // Only fails if a global logger was already set.
            eprintln!("svpn logger init failed: {e}");
            return;
        }
        log::set_max_level(LevelFilter::Debug);
    });
}

/// Point the file mirror at `<home_dir>/logs/svpn-tunnel.log`. Creates the
/// directory, rotates an oversized existing file to `.1`, and opens the file for
/// append. Called from `svpn_core_set_home_dir`. No-op on an empty path.
pub fn set_log_file(home_dir: &str) {
    if home_dir.is_empty() {
        return;
    }
    let dir = Path::new(home_dir).join("logs");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("svpn-tunnel.log");
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > MAX_LOG_BYTES {
            let _ = fs::rename(&path, dir.join("svpn-tunnel.log.1"));
        }
    }
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f) => {
            if let Ok(mut guard) = log_file().lock() {
                *guard = Some(f);
            }
        }
        Err(e) => eprintln!("svpn log file open failed: {e}"),
    }
}

/// Emit an internal lifecycle line at `info` level (so it reaches both os_log
/// and the mirrored file).
pub fn bridge_log(msg: &str) {
    log::info!("{msg}");
}

/// Route Rust panics to the logger before the runtime aborts.
///
/// With `panic = "abort"` a panic on a tokio worker takes the whole process
/// down, and NetworkExtension does not capture stderr — so without this hook
/// the iOS crash report shows only a backtrace, never the panic *message*.
/// Installing it once at `svpn_core_init` means any data-plane panic leaves a
/// readable line in os_log and the mirrored file. Idempotent.
pub fn install_panic_hook() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "<unknown>".to_string());
            let payload = info.payload();
            let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic payload>".to_string()
            };
            let thread = std::thread::current();
            let thread_name = thread.name().unwrap_or("<unnamed>");
            log::error!("rust panic in thread '{thread_name}' at {location}: {msg}");
            default_hook(info);
        }));
    });
}
