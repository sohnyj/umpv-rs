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

pub const DEFAULT_LOADFILE_MODE: &str = "replace";

pub fn encode_wide(string: &str) -> Vec<u16> {
    std::ffi::OsStr::new(string)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub enum Level {
    Error,
    Info,
    Warning,
}

pub fn show_message(level: Level, text: &str) {
    let prefix = match level {
        Level::Error => "Error",
        Level::Info => "Info",
        Level::Warning => "Warning",
    };
    let text_wide = encode_wide(&format!("{}: {}", prefix, text));
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

pub fn error_exit(text: &str) -> ! {
    show_message(Level::Error, text);
    process::exit(1);
}

fn parse_loadfile_mode(args: &[String]) -> Option<&str> {
    args.iter().find_map(|arg| arg.strip_prefix("--loadfile="))
}

fn resolve_file_path(arg: &str) -> String {
    if arg.contains("://") {
        return arg.to_string();
    }
    match std::path::absolute(arg) {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(_) => arg.to_string(),
    }
}

fn collect_files(args: &[String]) -> Vec<String> {
    args.iter()
        .filter(|arg| !arg.starts_with("--"))
        .map(|arg| resolve_file_path(arg))
        .collect()
}

fn main() {
    unsafe {
        windows_sys::Win32::UI::HiDpi::SetProcessDpiAwareness(
            windows_sys::Win32::UI::HiDpi::PROCESS_PER_MONITOR_DPI_AWARE,
        )
    };

    let args: Vec<String> = env::args().skip(1).collect();
    let loadfile_mode = parse_loadfile_mode(&args);

    match args.first().map(String::as_str) {
        Some("--register") => {
            registry::register(loadfile_mode);
            return;
        }
        Some("--unregister") => {
            registry::unregister();
            return;
        }
        _ => {}
    }

    if args.is_empty() {
        return;
    }

    let loadfile_mode = loadfile_mode.unwrap_or(DEFAULT_LOADFILE_MODE);

    let files = collect_files(&args);

    let _mutex_guard = match pipe::acquire_mutex() {
        Ok(guard) => guard,
        Err(MutexError::Timeout) => {
            error_exit("Failed to acquire lock: an mpv instance is not responding.")
        }
        Err(MutexError::Create) => error_exit("Failed to create umpv lock."),
    };

    match pipe::send_files(&files, loadfile_mode, false) {
        Ok(pid) => mpv::activate_mpv_window(pid),
        Err(SendError::Connect(ERROR_FILE_NOT_FOUND)) => {
            if let Err(err) = mpv::launch_mpv() {
                error_exit(&format!("Failed to launch mpv: {}", err));
            }
            if pipe::send_files(&files, loadfile_mode, true).is_err() {
                error_exit("Failed to send files to mpv.");
            }
        }
        Err(_) => error_exit("Failed to connect to mpv."),
    }
}
