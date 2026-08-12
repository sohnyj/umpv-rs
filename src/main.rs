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

fn show_information(text: &str) {
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

fn find_command(options: &[String]) -> Option<Command> {
    options.iter().find_map(|option| match option.as_str() {
        "--register" => Some(Command::Register),
        "--unregister" => Some(Command::Unregister),
        _ => None,
    })
}

const OPTION_PREFIX: &str = "--";

struct Arguments {
    options: Vec<String>,
    files: Vec<String>,
}

fn split_arguments(arguments: impl IntoIterator<Item = String>) -> Arguments {
    let mut options = Vec::new();
    let mut files = Vec::new();
    let mut past_end_of_options = false;

    for argument in arguments {
        if past_end_of_options || !argument.starts_with(OPTION_PREFIX) {
            files.push(argument);
        } else if argument == OPTION_PREFIX {
            past_end_of_options = true;
        } else {
            options.push(argument);
        }
    }

    Arguments { options, files }
}

fn resolve_file_path(file: &str) -> String {
    match std::path::absolute(file) {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(_) => file.to_string(),
    }
}

fn find_file_path(files: &[String]) -> Option<String> {
    files
        .iter()
        .find(|file| !file.is_empty())
        .map(|file| resolve_file_path(file))
}

const DEFAULT_LOADFILE: &str = "replace";

fn find_loadfile(options: &[String]) -> &str {
    options
        .iter()
        .find_map(|option| option.strip_prefix("--loadfile="))
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

fn register(requested_loadfile: &str, validated_loadfile: &str) {
    if validated_loadfile != requested_loadfile {
        warn_deprecated_loadfile(requested_loadfile, validated_loadfile);
    }
    let Ok(umpv_path) = env::current_exe() else {
        error_exit("Failed to locate umpv.exe.");
    };
    let command = format!(
        "\"{}\" --loadfile={validated_loadfile} -- \"%L\"",
        umpv_path.display()
    );

    match registry::register(&command) {
        Ok(count) => show_information(&format!(
            "umpv registered for {count} file extension(s).\nloadfile: {validated_loadfile}"
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
        0 => show_information("Nothing to unregister."),
        count => show_information(&format!("umpv unregistered for {count} file extension(s).")),
    }
}

fn play(files: &[String], loadfile: &str) {
    let Some(file) = find_file_path(files) else {
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
    let arguments = split_arguments(env::args().skip(1));
    let requested_loadfile = find_loadfile(&arguments.options);
    let validated_loadfile = validate_loadfile(requested_loadfile);

    match find_command(&arguments.options) {
        Some(Command::Register) => register(requested_loadfile, validated_loadfile),
        Some(Command::Unregister) => unregister(),
        None => play(&arguments.files, validated_loadfile),
    }
}
