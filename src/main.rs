#![windows_subsystem = "windows"]

use std::env;
use std::os::windows::ffi::OsStrExt;
use std::process;

use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;

use pipe::{MutexError, SendError};

mod mpv;
mod pipe;
mod registry;

fn encode_wide(string: &str) -> Vec<u16> {
    std::ffi::OsStr::new(string)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

enum Level {
    Error,
    Info,
    Warning,
}

fn show_message(level: Level, text: &str) {
    let prefix = match level {
        Level::Error => "Error",
        Level::Info => "Info",
        Level::Warning => "Warning",
    };
    let text_wide = encode_wide(&format!("{prefix}: {text}"));
    let caption_wide = encode_wide("umpv");
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text_wide.as_ptr(),
            caption_wide.as_ptr(),
            0,
        );
    }
}

fn error_exit(text: &str) -> ! {
    show_message(Level::Error, text);
    process::exit(1);
}

fn parse_option<'a>(args: &'a [String], prefix: &str) -> Option<&'a str> {
    args.iter().find_map(|arg| arg.strip_prefix(prefix))
}

fn resolve_file_path(arg: &str) -> String {
    match std::path::absolute(arg) {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(_) => arg.to_string(),
    }
}

fn find_file(args: &[String]) -> Option<String> {
    args.iter()
        .find(|arg| !arg.starts_with("--"))
        .map(|arg| resolve_file_path(arg))
}

const DEFAULT_LOADFILE: &str = "replace";
const DEFAULT_IDLESCREEN: &str = "no";

fn main() {
    unsafe {
        windows_sys::Win32::UI::HiDpi::SetProcessDpiAwareness(
            windows_sys::Win32::UI::HiDpi::PROCESS_PER_MONITOR_DPI_AWARE,
        )
    };

    let args: Vec<String> = env::args().skip(1).collect();
    let loadfile = parse_option(&args, "--loadfile=");
    let idlescreen = parse_option(&args, "--idlescreen=");

    match args.first().map(String::as_str) {
        Some("--register") => {
            registry::register(loadfile, idlescreen);
            return;
        }
        Some("--unregister") => {
            registry::unregister();
            return;
        }
        _ => {}
    }

    let Some(file) = find_file(&args) else {
        return;
    };

    let loadfile = loadfile.unwrap_or(DEFAULT_LOADFILE);
    let idlescreen = idlescreen.unwrap_or(DEFAULT_IDLESCREEN);

    let _mutex_guard = match pipe::acquire_mutex() {
        Ok(guard) => guard,
        Err(MutexError::Timeout) => {
            error_exit("Failed to acquire lock: an mpv instance is not responding.")
        }
        Err(MutexError::Create) => error_exit("Failed to create umpv lock."),
    };

    match pipe::send_file(&file, loadfile, false) {
        Ok(pid) => mpv::activate_mpv_window(pid),
        Err(SendError::Connect(ERROR_FILE_NOT_FOUND)) => {
            if let Err(err) = mpv::launch_mpv(idlescreen) {
                error_exit(&format!("Failed to launch mpv: {err}"));
            }
            if pipe::send_file(&file, loadfile, true).is_err() {
                error_exit("Failed to send the file to mpv.");
            }
        }
        Err(_) => error_exit("Failed to connect to mpv."),
    }
}
