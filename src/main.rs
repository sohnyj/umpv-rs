#![windows_subsystem = "windows"]

use std::env;
use std::fmt;
use std::path::PathBuf;
use std::process;

use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;
use windows_sys::core::w;

mod lock;
mod mpv;
mod pipe;
mod registry;

fn encode_wide(string: &str) -> Vec<u16> {
    string.encode_utf16().chain(std::iter::once(0)).collect()
}

fn show_message(text: &str) {
    let text_wide = encode_wide(text);
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text_wide.as_ptr(), w!("umpv"), 0);
    }
}

fn show_information(text: impl fmt::Display) {
    show_message(&format!("Info\n{text}"));
}

fn error_exit(text: impl fmt::Display) -> ! {
    show_message(&format!("Error\n{text}"));
    process::exit(1);
}

enum Command {
    Register,
    Unregister,
}

enum CommandLineOption {
    Command(Command),
    Loadfile(String),
}

const LOADFILE_OPTION_PREFIX: &str = "--loadfile=";
const SUPPORTED_LOADFILE_FLAGS: [&str; 5] = [
    "replace",
    "append",
    "append+play",
    "insert-next",
    "insert-next+play",
];
const DEFAULT_LOADFILE_FLAGS: &str = SUPPORTED_LOADFILE_FLAGS[0];

fn parse_option(option: &str) -> Option<CommandLineOption> {
    match option {
        "--register" => Some(CommandLineOption::Command(Command::Register)),
        "--unregister" => Some(CommandLineOption::Command(Command::Unregister)),
        _ => option
            .strip_prefix(LOADFILE_OPTION_PREFIX)
            .map(|loadfile_flags| CommandLineOption::Loadfile(loadfile_flags.to_owned())),
    }
}

enum ArgumentError {
    UnknownOption(String),
    UnsupportedLoadfileFlags(String),
}

impl fmt::Display for ArgumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownOption(option) => write!(formatter, "Unknown option: {option}"),
            Self::UnsupportedLoadfileFlags(loadfile_flags) => {
                write!(formatter, "Unsupported loadfile flags: {loadfile_flags}")
            }
        }
    }
}

struct Arguments {
    command: Option<Command>,
    loadfile_flags: String,
    files: Vec<String>,
}

fn parse_arguments(
    arguments: impl IntoIterator<Item = String>,
) -> Result<Arguments, ArgumentError> {
    let mut command = None;
    let mut loadfile_flags = None;
    let mut files = Vec::new();
    let mut past_end_of_options = false;

    for argument in arguments {
        if past_end_of_options || !argument.starts_with("--") {
            files.push(argument);
        } else if argument == "--" {
            past_end_of_options = true;
        } else {
            match parse_option(&argument) {
                Some(CommandLineOption::Command(parsed)) => command = command.or(Some(parsed)),
                Some(CommandLineOption::Loadfile(parsed)) => {
                    loadfile_flags = loadfile_flags.or(Some(parsed));
                }
                None => return Err(ArgumentError::UnknownOption(argument)),
            }
        }
    }

    let loadfile_flags = loadfile_flags.unwrap_or_else(|| DEFAULT_LOADFILE_FLAGS.to_owned());
    if !SUPPORTED_LOADFILE_FLAGS.contains(&loadfile_flags.as_str()) {
        return Err(ArgumentError::UnsupportedLoadfileFlags(loadfile_flags));
    }

    Ok(Arguments {
        command,
        loadfile_flags,
        files,
    })
}

fn has_url_scheme(argument: &str) -> bool {
    let Some((scheme, _)) = argument.split_once("://") else {
        return false;
    };
    scheme.starts_with(|character: char| character.is_ascii_alphabetic())
        && scheme.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
}

fn absolute_file_path(file: &str) -> String {
    match std::path::absolute(file) {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(error) => error_exit(format!("Failed to make the file path absolute: {error}")),
    }
}

fn umpv_path() -> PathBuf {
    let Ok(path) = env::current_exe() else {
        error_exit("Failed to locate umpv.exe.");
    };
    path
}

/// mpv is expected to sit next to umpv.
fn mpv_path() -> PathBuf {
    umpv_path().with_file_name("mpv.exe")
}

/// The shell open command that hands a picked file (`%L`) to this umpv.
fn umpv_command_line(loadfile_flags: &str) -> String {
    format!(
        "\"{}\" {LOADFILE_OPTION_PREFIX}{loadfile_flags} -- \"%L\"",
        umpv_path().display()
    )
}

fn register(loadfile_flags: &str) {
    match registry::register(&umpv_command_line(loadfile_flags)) {
        Ok(extension_count) => show_information(format!(
            "Registered for {extension_count} file extension(s).\nloadfile: {loadfile_flags}"
        )),
        Err(error) => error_exit(error),
    }
}

fn unregister() {
    match registry::unregister() {
        registry::Unregistered::Nothing => show_information("Nothing to unregister."),
        registry::Unregistered::ProgIdOnly => {
            show_information("Removed the umpv ProgID.\nNo file extensions were pointing at umpv.")
        }
        registry::Unregistered::Extensions(extension_count) => show_information(format!(
            "Unregistered for {extension_count} file extension(s)."
        )),
    }
}

fn launch_mpv(pipe: &pipe::Pipe, file: &str) {
    if let Err(error) = mpv::launch(&mpv_path(), pipe, file) {
        error_exit(error);
    }
}

/// How a file reached mpv.
enum Opened {
    /// An already running mpv instance accepted the file. Its process id is
    /// absent when that instance could not be identified.
    Existing { server_pid: Option<u32> },
    /// A new mpv instance was launched for the file.
    Launched,
}

fn open_in_mpv(pipe: &pipe::Pipe, file: &str, loadfile_flags: &str) -> Opened {
    let _lock_guard = match lock::acquire() {
        Ok(guard) => guard,
        Err(error) => error_exit(error),
    };

    match pipe.send_loadfile(file, loadfile_flags) {
        Ok(pipe::Sent::Delivered { server_pid }) => Opened::Existing { server_pid },
        Ok(pipe::Sent::NoServer) => {
            launch_mpv(pipe, file);
            Opened::Launched
        }
        Err(error) => error_exit(error),
    }
}

fn open(files: &[String], loadfile_flags: &str) {
    let Some(file) = files.first() else {
        return;
    };
    if has_url_scheme(file) {
        error_exit("URLs are not supported.\nOnly local files can be opened.");
    }
    let file = absolute_file_path(file);

    let pipe = match pipe::Pipe::for_current_session() {
        Ok(pipe) => pipe,
        Err(error) => error_exit(error),
    };

    if let Opened::Existing {
        server_pid: Some(server_pid),
    } = open_in_mpv(&pipe, &file, loadfile_flags)
    {
        mpv::activate_window(server_pid);
    }
}

fn main() {
    let arguments = match parse_arguments(env::args().skip(1)) {
        Ok(arguments) => arguments,
        Err(error) => error_exit(error),
    };

    match arguments.command {
        Some(Command::Register) => register(&arguments.loadfile_flags),
        Some(Command::Unregister) => unregister(),
        None => open(&arguments.files, &arguments.loadfile_flags),
    }
}
