#![windows_subsystem = "windows"]

use std::env;
use std::os::windows::ffi::OsStrExt;
use std::process;

use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;

mod lock;
mod mpv;
mod pipe;
mod registry;

fn encode_wide(string: &str) -> Vec<u16> {
    std::ffi::OsStr::new(string)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[derive(Clone, Copy)]
enum MessageLevel {
    Error,
    Info,
    Warning,
}

fn show_message(level: MessageLevel, text: &str) {
    let prefix = match level {
        MessageLevel::Error => "Error",
        MessageLevel::Info => "Info",
        MessageLevel::Warning => "Warning",
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
    show_message(MessageLevel::Error, text);
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
const DEFAULT_IDLESCREEN: &str = "no";

fn find_option_value<'a>(args: &'a [String], prefix: &str) -> Option<&'a str> {
    args.iter().find_map(|arg| arg.strip_prefix(prefix))
}

fn find_loadfile(args: &[String]) -> &str {
    find_option_value(args, "--loadfile=").unwrap_or(DEFAULT_LOADFILE)
}

fn find_idlescreen(args: &[String]) -> &str {
    find_option_value(args, "--idlescreen=").unwrap_or(DEFAULT_IDLESCREEN)
}

fn warn_deprecated_loadfile(deprecated: &str, replacement: &str) {
    show_message(
        MessageLevel::Warning,
        &format!("'{deprecated}' is deprecated since mpv 0.42.\nUsing '{replacement}' instead."),
    );
}

fn validate_loadfile(loadfile: &str) -> &str {
    match loadfile {
        "replace" | "append" | "append+play" | "insert-next" | "insert-next+play" => loadfile,
        "append-play" => "append+play",
        "insert-next-play" => "insert-next+play",
        _ => error_exit(&format!("Unsupported loadfile flag: {loadfile}")),
    }
}

fn validate_idlescreen(idlescreen: &str) -> &str {
    if !matches!(idlescreen, "yes" | "no") {
        error_exit(&format!(
            "Unsupported idlescreen value: {idlescreen}\nUse 'yes' or 'no'."
        ));
    }
    idlescreen
}

fn register(args: &[String], loadfile: &str, idlescreen: &str) {
    let given_loadfile = find_loadfile(args);
    if loadfile != given_loadfile {
        warn_deprecated_loadfile(given_loadfile, loadfile);
    }
    match registry::register(loadfile, idlescreen) {
        Ok(count) => show_message(
            MessageLevel::Info,
            &format!(
                "umpv registered for {count} file extension(s).\nloadfile: {loadfile}\nidlescreen: {idlescreen}"
            ),
        ),
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
        0 => show_message(MessageLevel::Info, "Nothing to unregister."),
        count => show_message(
            MessageLevel::Info,
            &format!("umpv unregistered for {count} file extension(s)."),
        ),
    }
}

fn play(args: &[String], loadfile: &str, idlescreen: &str) {
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
        Err(pipe::Error::NotRunning) => {
            match mpv::launch(idlescreen, &file) {
                Ok(()) => {}
                Err(mpv::Error::NotFound) => error_exit("Failed to launch mpv: mpv.exe not found."),
                Err(mpv::Error::SpawnFailed(err)) => {
                    error_exit(&format!("Failed to launch mpv: {err}"))
                }
            }
            pipe::wait_for_server();
        }
        Err(pipe::Error::ConnectFailed) => error_exit("Failed to connect to mpv."),
        Err(pipe::Error::WriteFailed) => error_exit("Failed to send the file to mpv."),
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let loadfile = validate_loadfile(find_loadfile(&args));
    let idlescreen = validate_idlescreen(find_idlescreen(&args));

    match find_command(&args) {
        Some(Command::Register) => register(&args, loadfile, idlescreen),
        Some(Command::Unregister) => unregister(),
        None => play(&args, loadfile, idlescreen),
    }
}
