use std::fmt;
use std::ptr;

use windows_registry::{CURRENT_USER, Key};
use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};

pub(crate) enum Error {
    NoAssociations,
    ProgIdWriteFailed,
    NoExtensionsRegistered,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NoAssociations => {
                "No mpv file associations found.\nRun 'mpv.exe --register' first."
            }
            Self::ProgIdWriteFailed => "Failed to write umpv ProgID to registry.",
            Self::NoExtensionsRegistered => "Failed to register any file associations.",
        })
    }
}

const SUBKEY_FILE_ASSOCIATIONS: &str = r"Software\Clients\Media\mpv\Capabilities\FileAssociations";
const SUBKEY_CLASSES: &str = r"Software\Classes";
const UMPV_PROG_ID: &str = "io.mpv.umpv";
const MPV_PROG_ID: &str = "io.mpv.file";
/// A registry key's unnamed default value.
const DEFAULT_VALUE_NAME: &str = "";

fn umpv_prog_id_subkey() -> String {
    format!(r"{SUBKEY_CLASSES}\{UMPV_PROG_ID}")
}

fn notify_shell_change() {
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED.cast_signed(),
            SHCNF_IDLIST,
            ptr::null(),
            ptr::null(),
        );
    }
}

struct FileAssociation {
    extension: String,
    prog_id: String,
}

fn open_file_associations() -> Option<Key> {
    CURRENT_USER
        .options()
        .read()
        .write()
        .open(SUBKEY_FILE_ASSOCIATIONS)
        .ok()
}

fn read_associations(key: &Key) -> Vec<FileAssociation> {
    let Ok(values) = key.values() else {
        return Vec::new();
    };
    values
        .filter(|(name, _)| name.starts_with('.') && name.len() > 1)
        .filter_map(|(name, value)| {
            Some(FileAssociation {
                extension: name,
                prog_id: String::try_from(value).ok()?,
            })
        })
        .collect()
}

fn write_prog_id(shell_open_command: &str) -> windows_registry::Result<()> {
    let prog_id_key = CURRENT_USER.create(umpv_prog_id_subkey())?;
    // Empty so the shell shows the extension instead of this ProgID's name.
    prog_id_key.set_string(DEFAULT_VALUE_NAME, "")?;
    prog_id_key
        .create(r"shell\open\command")?
        .set_string(DEFAULT_VALUE_NAME, shell_open_command)
}

fn set_associations<'a>(
    key: &Key,
    associations: impl IntoIterator<Item = &'a FileAssociation>,
    prog_id: &str,
) -> usize {
    let mut count = 0;
    for association in associations {
        if key.set_string(&association.extension, prog_id).is_ok() {
            count += 1;
        }
    }
    count
}

pub(crate) fn register(shell_open_command: &str) -> Result<usize, Error> {
    let key = open_file_associations().ok_or(Error::NoAssociations)?;
    let associations = read_associations(&key);
    if associations.is_empty() {
        return Err(Error::NoAssociations);
    }

    write_prog_id(shell_open_command).map_err(|_| Error::ProgIdWriteFailed)?;

    let extension_count = set_associations(&key, &associations, UMPV_PROG_ID);
    if extension_count == 0 {
        return Err(Error::NoExtensionsRegistered);
    }

    notify_shell_change();
    Ok(extension_count)
}

pub(crate) enum Unregistered {
    Nothing,
    ProgIdOnly,
    ExtensionsRestored(usize),
}

pub(crate) fn unregister() -> Unregistered {
    let extension_count = open_file_associations().map_or(0, |key| {
        let associations = read_associations(&key);
        set_associations(
            &key,
            associations
                .iter()
                .filter(|association| association.prog_id == UMPV_PROG_ID),
            MPV_PROG_ID,
        )
    });
    let removed_prog_id = CURRENT_USER.remove_tree(umpv_prog_id_subkey()).is_ok();

    let unregistered = match (extension_count, removed_prog_id) {
        (0, false) => Unregistered::Nothing,
        (0, true) => Unregistered::ProgIdOnly,
        _ => Unregistered::ExtensionsRestored(extension_count),
    };
    if !matches!(unregistered, Unregistered::Nothing) {
        notify_shell_change();
    }
    unregistered
}
