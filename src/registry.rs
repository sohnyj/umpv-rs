use windows_sys::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegEnumValueW, RegOpenKeyExW, RegSetValueExW,
};
use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};

use crate::{
    DEFAULT_IDLESCREEN, DEFAULT_LOADFILE_MODE, Level, encode_wide, error_exit, show_message,
};

const SUBKEY_FILE_ASSOCIATIONS: &str = r"Software\Clients\Media\mpv\Capabilities\FileAssociations";
const SUBKEY_UMPV_PROG_ID: &str = r"Software\Classes\io.mpv.umpv";
const UMPV_PROG_ID: &str = "io.mpv.umpv";
const MPV_PROG_ID: &str = "io.mpv.file";

fn notify_shell_change() {
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        );
    }
}

fn read_values(key: HKEY, sub_key: &str) -> Vec<(String, String)> {
    let sub_key_wide = encode_wide(sub_key);
    let mut results = Vec::new();
    unsafe {
        let mut opened_key: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(key, sub_key_wide.as_ptr(), 0, KEY_READ, &mut opened_key) as u32
            != ERROR_SUCCESS
        {
            return results;
        }

        let mut index: u32 = 0;
        loop {
            let mut name_buffer = [0u16; 256];
            let mut name_length: u32 = 256;
            let mut data_buffer = [0u16; 1024];
            let mut data_length = std::mem::size_of_val(&data_buffer) as u32;
            let mut value_type: u32 = 0;

            let status = RegEnumValueW(
                opened_key,
                index,
                name_buffer.as_mut_ptr(),
                &mut name_length,
                std::ptr::null_mut(),
                &mut value_type,
                data_buffer.as_mut_ptr() as *mut u8,
                &mut data_length,
            ) as u32;

            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status != ERROR_SUCCESS {
                index += 1;
                continue;
            }

            if value_type == REG_SZ && name_length > 0 {
                let name = String::from_utf16_lossy(&name_buffer[..name_length as usize]);
                let data_char_count = data_length as usize / std::mem::size_of::<u16>();
                let data = if data_char_count > 0 && data_buffer[data_char_count - 1] == 0 {
                    String::from_utf16_lossy(&data_buffer[..data_char_count - 1])
                } else {
                    String::from_utf16_lossy(&data_buffer[..data_char_count])
                };
                results.push((name, data));
            }
            index += 1;
        }
        RegCloseKey(opened_key);
    }
    results
}

fn read_assocs(key: HKEY, sub_key: &str) -> Vec<(String, String)> {
    read_values(key, sub_key)
        .into_iter()
        .filter(|(name, _)| name.starts_with('.') && name.len() > 1)
        .collect()
}

fn create_or_open_key(key: HKEY, sub_key: &str) -> Option<HKEY> {
    let sub_key_wide = encode_wide(sub_key);
    unsafe {
        let mut opened_key: HKEY = std::ptr::null_mut();
        if RegCreateKeyExW(
            key,
            sub_key_wide.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &mut opened_key,
            std::ptr::null_mut(),
        ) as u32
            != ERROR_SUCCESS
        {
            return None;
        }
        Some(opened_key)
    }
}

fn write_value(opened_key: HKEY, name: Option<&str>, data: &str) -> bool {
    let data_wide = encode_wide(data);
    let name_wide;
    let name_ptr = match name {
        Some(name_string) => {
            name_wide = encode_wide(name_string);
            name_wide.as_ptr()
        }
        None => std::ptr::null(),
    };
    unsafe {
        RegSetValueExW(
            opened_key,
            name_ptr,
            0,
            REG_SZ,
            data_wide.as_ptr() as *const u8,
            (data_wide.len() * std::mem::size_of::<u16>()) as u32,
        ) as u32
            == ERROR_SUCCESS
    }
}

fn set_value(key: HKEY, sub_key: &str, name: Option<&str>, data: &str) -> bool {
    let Some(opened_key) = create_or_open_key(key, sub_key) else {
        return false;
    };
    let success = write_value(opened_key, name, data);
    unsafe { RegCloseKey(opened_key) };
    success
}

fn set_assocs(exts: impl IntoIterator<Item = impl AsRef<str>>, prog_id: &str) -> usize {
    let Some(opened_key) = create_or_open_key(HKEY_CURRENT_USER, SUBKEY_FILE_ASSOCIATIONS) else {
        return 0;
    };
    let mut count = 0;
    for ext in exts {
        if write_value(opened_key, Some(ext.as_ref()), prog_id) {
            count += 1;
        }
    }
    unsafe { RegCloseKey(opened_key) };
    count
}

fn delete_tree(key: HKEY, sub_key: &str) {
    let sub_key_wide = encode_wide(sub_key);
    unsafe { RegDeleteTreeW(key, sub_key_wide.as_ptr()) };
}

pub fn register(loadfile_mode: Option<&str>, idlescreen: Option<&str>) {
    let assocs = read_assocs(HKEY_CURRENT_USER, SUBKEY_FILE_ASSOCIATIONS);
    if assocs.is_empty() {
        error_exit("No mpv file associations found.\nRun 'mpv.exe --register' first.");
    }

    let umpv_path = std::env::current_exe().expect("umpv.exe path");
    let loadfile_mode = loadfile_mode.unwrap_or(DEFAULT_LOADFILE_MODE);
    let idlescreen = idlescreen.unwrap_or(DEFAULT_IDLESCREEN);

    if !matches!(idlescreen, "yes" | "no") {
        error_exit(&format!(
            "Unsupported idlescreen value: {}\nUse 'yes' or 'no'.",
            idlescreen
        ));
    }

    if !matches!(
        loadfile_mode,
        "replace"
            | "append"
            | "append+play"
            | "append-play"
            | "insert-next"
            | "insert-next+play"
            | "insert-next-play"
    ) {
        error_exit(&format!("Unsupported loadfile flag: {}", loadfile_mode));
    }

    let loadfile_mode = if matches!(loadfile_mode, "append-play" | "insert-next-play") {
        let replacement = loadfile_mode.replace("-play", "+play");
        show_message(
            Level::Warning,
            &format!(
                "'{}' is deprecated since mpv 0.42.\nUsing '{}' instead.",
                loadfile_mode, replacement
            ),
        );
        replacement
    } else {
        loadfile_mode.to_string()
    };

    let command = format!(
        "\"{}\" --loadfile={} --idlescreen={} -- \"%L\"",
        umpv_path.display(),
        loadfile_mode,
        idlescreen
    );
    let command_key = format!("{}\\shell\\open\\command", SUBKEY_UMPV_PROG_ID);
    if !set_value(HKEY_CURRENT_USER, SUBKEY_UMPV_PROG_ID, None, "")
        || !set_value(HKEY_CURRENT_USER, &command_key, None, &command)
    {
        error_exit("Failed to write umpv ProgID to registry.");
    }

    let count = set_assocs(assocs.iter().map(|(ext, _)| ext), UMPV_PROG_ID);
    if count == 0 {
        error_exit("Failed to register any file associations.");
    }

    notify_shell_change();
    show_message(
        Level::Info,
        &format!(
            "umpv registered for {} file extension(s).\nloadfile: {}\nidlescreen: {}",
            count, loadfile_mode, idlescreen
        ),
    );
}

pub fn unregister() {
    let assocs = read_assocs(HKEY_CURRENT_USER, SUBKEY_FILE_ASSOCIATIONS);

    let umpv_assocs: Vec<_> = assocs
        .iter()
        .filter(|(_, data)| data == UMPV_PROG_ID)
        .collect();

    if umpv_assocs.is_empty() {
        show_message(Level::Info, "Nothing to unregister.");
        return;
    }

    let count = set_assocs(umpv_assocs.iter().map(|(ext, _)| ext), MPV_PROG_ID);

    delete_tree(HKEY_CURRENT_USER, SUBKEY_UMPV_PROG_ID);

    notify_shell_change();
    show_message(
        Level::Info,
        &format!("umpv unregistered for {} file extension(s).", count),
    );
}
