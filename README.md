# umpv-rust

A single-instance mpv launcher for Windows, written in Rust. Based on the [umpv](https://github.com/mpv-player/mpv/blob/master/TOOLS/umpv) Python script from the mpv project. Opens files in a running mpv window via named pipe IPC, or launches a new instance if none is running.

## Minimum Requirements

- OS: Windows 10+ (x64)

## Usage

**Place umpv.exe in the same directory as mpv.exe**

### 1. Register file associations with mpv ([mpv-register helper](https://mpv.io/manual/master/#options-register))

```bat
.\mpv.com --register --video-exts=mp4,mkv --audio-exts= --image-exts= --archive-exts= --playlist-exts=
```

Specify the extensions you want. Leave a category empty (`=`) to skip it.

> [!NOTE]
> **Running the register command with Administrator privileges is not recommended,** as umpv does not support system-wide associations.

### 2. Add umpv to mpv's registered extensions

Only processes extensions that were registered by the mpv-register helper (step 1). 

```bat
.\umpv.exe --register
```

> [!NOTE]
> umpv only supports per-user file associations (`HKEY_CURRENT_USER`). **Running as administrator is neither required nor supported, and umpv does not support system-wide associations.**
> To set umpv as the default for each extension, go to Windows Settings > App > Default apps > mpv, and select umpv for the desired extensions.

Without arguments, registers with the default loadfile mode (`replace`). Optionally specify a different mode:

```bat
.\umpv.exe --register --loadfile=append+play
```

### 3. Unregister umpv

```bat
.\umpv.exe --unregister
```

Removes umpv file associations from the registry. Does not restore previous defaults.

## Options

The `--loadfile=<value>` option controls how files are added to the mpv playlist.

| Value | Description |
|-------|-------------|
| `replace` | Stop current playback and play the new file (default) |
| `append` | Append to the end of the playlist |
| `append+play` | Append, and force playback to start |
| `insert-next` | Insert after the current item |
| `insert-next+play` | Insert after the current item, and force playback to start |

The following flags (deprecated since mpv 0.42) are also accepted:

| Value | Description |
|-------|-------------|
| `append-play` | Equivalent to `append+play` |
| `insert-next-play` | Equivalent to `insert-next+play` |

The following flags are not supported:

| Value | Description |
|-------|-------------|
| `insert-at` | umpv alone cannot determine the playlist index |
| `insert-at+play` | umpv alone cannot determine the playlist index |

URLs (`scheme://...`) are passed through to mpv as-is without path resolution.

See the [mpv documentation](https://mpv.io/manual/master/#command-interface-[%3Coptions%3E]]]) for the full list of options.

## Cross-compiling

### Requirements

- [Rust](https://www.rust-lang.org/tools/install) (stable)
- [cargo-xwin](https://github.com/rust-cross/cargo-xwin): cross-compiles to Windows MSVC target on Linux/macOS
- `llvm-rc`: included in the LLVM package (`llvm` on apt, `llvm` on Homebrew)

```bash
cargo install cargo-xwin
```

### Build

```bash
cargo xwin build --release
```

Output: `target/x86_64-pc-windows-msvc/release/umpv.exe`

## Acknowledgements

`mpv-icon.ico` is property of the [mpv project](https://github.com/mpv-player/mpv).
