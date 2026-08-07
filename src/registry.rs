use windows_registry::{CURRENT_USER, Type};
use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};

use crate::{MessageLevel, error_exit, show_message};

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
        .filter_map(|(name, value)| match value.ty() {
            Type::String => Some((name, String::try_from(value).ok()?)),
            _ => None,
        })
        .collect()
}

fn write_prog_id(command: &str) -> windows_registry::Result<()> {
    let prog_id_key = CURRENT_USER.create(SUBKEY_UMPV_PROG_ID)?;
    prog_id_key.set_string("", "")?;
    prog_id_key
        .create(r"shell\open\command")?
        .set_string("", command)
}

fn set_associations(extensions: impl IntoIterator<Item = impl AsRef<str>>, prog_id: &str) -> usize {
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

pub(crate) fn register(loadfile: &str, idlescreen: &str) {
    let associations = read_associations();
    if associations.is_empty() {
        error_exit("No mpv file associations found.\nRun 'mpv.exe --register' first.");
    }

    let umpv_path = std::env::current_exe().expect("umpv.exe path");
    let command = format!(
        "\"{}\" --loadfile={} --idlescreen={} -- \"%L\"",
        umpv_path.display(),
        loadfile,
        idlescreen
    );
    if write_prog_id(&command).is_err() {
        error_exit("Failed to write umpv ProgID to registry.");
    }

    let count = set_associations(
        associations.iter().map(|(extension, _)| extension),
        UMPV_PROG_ID,
    );
    if count == 0 {
        error_exit("Failed to register any file associations.");
    }

    notify_shell_change();
    show_message(
        MessageLevel::Info,
        &format!(
            "umpv registered for {count} file extension(s).\nloadfile: {loadfile}\nidlescreen: {idlescreen}"
        ),
    );
}

pub(crate) fn unregister() {
    let associations = read_associations();

    let umpv_associations: Vec<_> = associations
        .iter()
        .filter(|(_, data)| data == UMPV_PROG_ID)
        .collect();

    if umpv_associations.is_empty() {
        show_message(MessageLevel::Info, "Nothing to unregister.");
        return;
    }

    let count = set_associations(
        umpv_associations.iter().map(|(extension, _)| extension),
        MPV_PROG_ID,
    );

    let _ = CURRENT_USER.remove_tree(SUBKEY_UMPV_PROG_ID);

    notify_shell_change();
    show_message(
        MessageLevel::Info,
        &format!("umpv unregistered for {count} file extension(s)."),
    );
}
