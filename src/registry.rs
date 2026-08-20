use windows_registry::CURRENT_USER;
use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};

pub(crate) enum Error {
    NoAssociations,
    ProgIdWriteFailed,
    NoExtensionsRegistered,
}

const SUBKEY_FILE_ASSOCIATIONS: &str = r"Software\Clients\Media\mpv\Capabilities\FileAssociations";
const SUBKEY_UMPV_PROG_ID: &str = r"Software\Classes\io.mpv.umpv";
const UMPV_PROG_ID: &str = "io.mpv.umpv";
const MPV_PROG_ID: &str = "io.mpv.file";

fn notify_shell_change() {
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED.cast_signed(),
            SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        );
    }
}

fn read_associations() -> Vec<(String, String)> {
    let Ok(key) = CURRENT_USER.open(SUBKEY_FILE_ASSOCIATIONS) else {
        return Vec::new();
    };
    let Ok(values) = key.values() else {
        return Vec::new();
    };
    values
        .filter(|(name, _)| name.starts_with('.') && name.len() > 1)
        .filter_map(|(name, value)| Some((name, String::try_from(value).ok()?)))
        .collect()
}

fn write_prog_id(command: &str) -> windows_registry::Result<()> {
    let prog_id_key = CURRENT_USER.create(SUBKEY_UMPV_PROG_ID)?;
    prog_id_key.set_string("", "")?;
    prog_id_key
        .create(r"shell\open\command")?
        .set_string("", command)
}

fn set_associations<'a>(extensions: impl IntoIterator<Item = &'a str>, prog_id: &str) -> usize {
    let Ok(key) = CURRENT_USER.create(SUBKEY_FILE_ASSOCIATIONS) else {
        return 0;
    };
    let mut count = 0;
    for extension in extensions {
        if key.set_string(extension, prog_id).is_ok() {
            count += 1;
        }
    }
    count
}

pub(crate) fn register(command: &str) -> Result<usize, Error> {
    let associations = read_associations();
    if associations.is_empty() {
        return Err(Error::NoAssociations);
    }

    write_prog_id(command).map_err(|_| Error::ProgIdWriteFailed)?;

    let count = set_associations(
        associations.iter().map(|(extension, _)| extension.as_str()),
        UMPV_PROG_ID,
    );
    if count == 0 {
        return Err(Error::NoExtensionsRegistered);
    }

    notify_shell_change();
    Ok(count)
}

pub(crate) fn unregister() -> usize {
    let associations = read_associations();
    let umpv_extensions: Vec<&str> = associations
        .iter()
        .filter(|(_, prog_id)| prog_id == UMPV_PROG_ID)
        .map(|(extension, _)| extension.as_str())
        .collect();

    let count = if umpv_extensions.is_empty() {
        0
    } else {
        set_associations(umpv_extensions, MPV_PROG_ID)
    };
    let _ = CURRENT_USER.remove_tree(SUBKEY_UMPV_PROG_ID);

    notify_shell_change();
    count
}
