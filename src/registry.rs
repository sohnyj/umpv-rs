use std::fmt;
use std::ptr;

use windows_registry::{CURRENT_USER, Key, Value};
use windows_result::WIN32_ERROR;
use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::UI::Shell::{self, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

pub(crate) enum Error {
    NoAssociations,
    AssociationsReadFailed,
    ProgIdWriteFailed,
    NoExtensionsRegistered,
    ProgIdRemoveFailed,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NoAssociations => {
                "No mpv file associations found.\nRun 'mpv.exe --register' first."
            }
            Self::AssociationsReadFailed => "Failed to read the mpv file associations.",
            Self::ProgIdWriteFailed => "Failed to write umpv ProgID to registry.",
            Self::NoExtensionsRegistered => "Failed to register any file associations.",
            Self::ProgIdRemoveFailed => "Failed to remove umpv ProgID from registry.",
        })
    }
}

const SUBKEY_FILE_ASSOCIATIONS: &str = r"Software\Clients\Media\mpv\Capabilities\FileAssociations";
const SUBKEY_CLASSES: &str = r"Software\Classes";
const UMPV_PROG_ID: &str = "io.mpv.umpv";
const MPV_PROG_ID: &str = "io.mpv.file";
/// A registry key's unnamed default value.
const DEFAULT_VALUE_NAME: &str = "";

fn is_not_found(error: &windows_result::Error) -> bool {
    WIN32_ERROR::from_error(error) == Some(WIN32_ERROR(ERROR_FILE_NOT_FOUND))
}

fn umpv_prog_id_subkey() -> String {
    format!(r"{SUBKEY_CLASSES}\{UMPV_PROG_ID}")
}

fn notify_shell_change() {
    unsafe {
        Shell::SHChangeNotify(
            SHCNE_ASSOCCHANGED.cast_signed(),
            SHCNF_IDLIST,
            ptr::null(),
            ptr::null(),
        );
    }
}

fn open_file_associations() -> windows_registry::Result<Key> {
    CURRENT_USER
        .options()
        .read()
        .write()
        .open(SUBKEY_FILE_ASSOCIATIONS)
}

/// The ProgID comes by value, so a string is built only where one is compared.
fn read_extensions(
    key: &Key,
    accept_prog_id: impl Fn(Value) -> bool,
) -> windows_registry::Result<Vec<String>> {
    Ok(key
        .values()?
        .filter(|(name, _)| name.starts_with('.') && name.len() > 1)
        .filter_map(|(name, prog_id)| accept_prog_id(prog_id).then_some(name))
        .collect())
}

fn is_umpv_prog_id(prog_id: Value) -> bool {
    String::try_from(prog_id).is_ok_and(|prog_id| prog_id == UMPV_PROG_ID)
}

fn write_prog_id(shell_open_command: &str) -> windows_registry::Result<()> {
    let prog_id_key = CURRENT_USER.create(umpv_prog_id_subkey())?;
    // Empty so the shell shows the extension instead of this ProgID's name.
    prog_id_key.set_string(DEFAULT_VALUE_NAME, "")?;
    prog_id_key
        .create(r"shell\open\command")?
        .set_string(DEFAULT_VALUE_NAME, shell_open_command)
}

fn set_associations(key: &Key, extensions: &[String], prog_id: &str) -> usize {
    let mut count = 0;
    for extension in extensions {
        if key.set_string(extension, prog_id).is_ok() {
            count += 1;
        }
    }
    count
}

pub(crate) fn register(shell_open_command: &str) -> Result<usize, Error> {
    let key = open_file_associations().map_err(|error| {
        if is_not_found(&error) {
            Error::NoAssociations
        } else {
            Error::AssociationsReadFailed
        }
    })?;
    let extensions = read_extensions(&key, |_| true).map_err(|_| Error::AssociationsReadFailed)?;
    if extensions.is_empty() {
        return Err(Error::NoAssociations);
    }

    write_prog_id(shell_open_command).map_err(|_| Error::ProgIdWriteFailed)?;

    let extension_count = set_associations(&key, &extensions, UMPV_PROG_ID);
    if extension_count == 0 {
        return Err(Error::NoExtensionsRegistered);
    }

    notify_shell_change();
    Ok(extension_count)
}

pub(crate) enum Unregistered {
    Nothing,
    ProgIdOnly,
    Extensions(usize),
}

/// `false` when there was no ProgID left to remove.
fn remove_prog_id() -> Result<bool, Error> {
    match CURRENT_USER.remove_tree(umpv_prog_id_subkey()) {
        Ok(()) => Ok(true),
        Err(error) if is_not_found(&error) => Ok(false),
        Err(_) => Err(Error::ProgIdRemoveFailed),
    }
}

pub(crate) fn unregister() -> Result<Unregistered, Error> {
    let extension_count = match open_file_associations() {
        Ok(key) => {
            let extensions = read_extensions(&key, is_umpv_prog_id)
                .map_err(|_| Error::AssociationsReadFailed)?;
            set_associations(&key, &extensions, MPV_PROG_ID)
        }
        Err(error) if is_not_found(&error) => 0,
        Err(_) => return Err(Error::AssociationsReadFailed),
    };
    let removed_prog_id = remove_prog_id()?;

    let unregistered = match (extension_count, removed_prog_id) {
        (0, false) => Unregistered::Nothing,
        (0, true) => Unregistered::ProgIdOnly,
        _ => Unregistered::Extensions(extension_count),
    };
    if !matches!(unregistered, Unregistered::Nothing) {
        notify_shell_change();
    }
    Ok(unregistered)
}
