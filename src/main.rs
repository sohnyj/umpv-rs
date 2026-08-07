#![windows_subsystem = "windows"]

use std::env;
use std::process;

use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;

mod lock;
mod mpv;
mod pipe;
mod registry;

fn encode_wide(string: &str) -> Vec<u16> {
    string.encode_utf16().chain(std::iter::once(0)).collect()
}

fn show_message(prefix: &str, text: &str) {
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

fn show_info(text: &str) {
    show_message("Info", text);
}

fn show_warning(text: &str) {
    show_message("Warning", text);
}

fn error_exit(text: &str) -> ! {
    show_message("Error", text);
    process::exit(1);
}

enum Command {
    Register,
    Unregister,
}

fn find_command(args: &[String]) -> Option<Command> {
    args.iter().find_map(|arg| match arg.as_str() {
        "--register" => Some(Command::Register),
        "--unregister" => Some(Command::Unregister),
        _ => None,
    })
}

fn resolve_file_path(arg: &str) -> String {
    match std::path::absolute(arg) {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(_) => arg.to_string(),
    }
}

fn find_file_argument(args: &[String]) -> Option<String> {
    args.iter()
        .find(|arg| !arg.is_empty() && !arg.starts_with("--"))
        .map(|arg| resolve_file_path(arg))
}

const DEFAULT_LOADFILE: &str = "replace";

fn find_loadfile(args: &[String]) -> &str {
    args.iter()
        .find_map(|arg| arg.strip_prefix("--loadfile="))
        .unwrap_or(DEFAULT_LOADFILE)
}

fn warn_deprecated_loadfile(deprecated: &str, replacement: &str) {
    show_warning(&format!(
        "'{deprecated}' is deprecated since mpv 0.42.\nUsing '{replacement}' instead."
    ));
}

fn validate_loadfile(loadfile: &str) -> &str {
    match loadfile {
        "replace" | "append" | "append+play" | "insert-next" | "insert-next+play" => loadfile,
        "append-play" => "append+play",
        "insert-next-play" => "insert-next+play",
        _ => error_exit(&format!("Unsupported loadfile flag: {loadfile}")),
    }
}

fn register(requested_loadfile: &str, loadfile: &str) {
    if loadfile != requested_loadfile {
        warn_deprecated_loadfile(requested_loadfile, loadfile);
    }
    let Ok(umpv_path) = env::current_exe() else {
        error_exit("Failed to locate umpv.exe.");
    };
    let command = format!(
        "\"{}\" --loadfile={loadfile} -- \"%L\"",
        umpv_path.display()
    );

    match registry::register(&command) {
        Ok(count) => show_info(&format!(
            "umpv registered for {count} file extension(s).\nloadfile: {loadfile}"
        )),
        Err(registry::Error::NoAssociations) => {
            error_exit("No mpv file associations found.\nRun 'mpv.exe --register' first.")
        }
        Err(registry::Error::ProgIdWriteFailed) => {
            error_exit("Failed to write umpv ProgID to registry.")
        }
        Err(registry::Error::NoExtensionsRegistered) => {
            error_exit("Failed to register any file associations.")
        }
    }
}

fn unregister() {
    match registry::unregister() {
        0 => show_info("Nothing to unregister."),
        count => show_info(&format!("umpv unregistered for {count} file extension(s).")),
    }
}

fn play(args: &[String], loadfile: &str) {
    let Some(file) = find_file_argument(args) else {
        return;
    };

    let _lock_guard = match lock::acquire() {
        Ok(guard) => guard,
        Err(lock::Error::Timeout) => {
            error_exit("Failed to acquire lock: an mpv instance is not responding.")
        }
        Err(lock::Error::CreateFailed) => error_exit("Failed to create umpv lock."),
    };

    match pipe::send_file(&file, loadfile) {
        Ok(pid) => mpv::activate_window(pid),
        Err(pipe::Error::NotRunning) => match mpv::launch(&file) {
            Ok(()) => pipe::wait_for_server(),
            Err(mpv::Error::UmpvPathUnknown) => error_exit("Failed to locate umpv.exe."),
            Err(mpv::Error::SpawnFailed(err)) => {
                error_exit(&format!("Failed to launch mpv.exe: {err}"))
            }
        },
        Err(pipe::Error::ConnectFailed) => error_exit("Failed to connect to mpv."),
        Err(pipe::Error::WriteFailed) => error_exit("Failed to send the file to mpv."),
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let requested_loadfile = find_loadfile(&args);
    let loadfile = validate_loadfile(requested_loadfile);

    match find_command(&args) {
        Some(Command::Register) => register(requested_loadfile, loadfile),
        Some(Command::Unregister) => unregister(),
        None => play(&args, loadfile),
    }
}
