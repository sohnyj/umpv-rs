# umpv-rs

A single-instance mpv launcher for Windows, written in Rust. Opens files in a running mpv window via named pipe IPC, or launches a new instance if none is running.

## Minimum Requirements

- OS: Windows 10+ (x64)
- CPU: x86_64-v3 (AVX2 support required)

## Usage

**Place `umpv.exe` next to `mpv.exe`.** umpv launches `mpv.exe` from its own directory, so PATH is not consulted.

### 1. Register extensions with mpv

Specify the extensions you want; leave a category empty (`=`) to skip it. See [mpv's `--register`](https://mpv.io/manual/master/#options-register).

```bat
.\mpv.com --register --video-exts=mp4,mkv --audio-exts= --image-exts= --archive-exts= --playlist-exts=
```

### 2. Point those extensions at umpv

```bat
.\umpv.exe --register --loadfile=append+play
```

Only extensions registered in step 1 are processed. `--loadfile=` is optional and defaults to `replace`.

> [!NOTE]
> Per-user associations only (`HKEY_CURRENT_USER`); administrator is neither required nor supported.
> To make umpv the default for each extension, go to Windows Settings > Apps > Default apps > mpv, and select umpv.

### 3. Unregister

```bat
.\umpv.exe --unregister
```

Points the extensions back at mpv. Defaults set by other applications are not restored.

## Loadfile modes

`--loadfile=<value>` controls how files are added to the mpv playlist. Set at registration time.

| Value | Description |
|-------|-------------|
| `replace` | Stop current playback and play the new file (default) |
| `append` | Append to the end of the playlist |
| `append+play` | Append, and force playback to start |
| `insert-next` | Insert after the current item |
| `insert-next+play` | Insert after the current item, and force playback to start |

`insert-at` and `insert-at+play` are not supported, as umpv alone cannot determine the playlist index. See the [mpv documentation](https://mpv.io/manual/master/#command-interface-[%3Coptions%3E]]]) for the full list.

## Cross-compiling

umpv is built from Linux (including WSL) for the `x86_64-pc-windows-msvc` target. CI uses the same toolchain ([build.yml](.github/workflows/build.yml)).

```bash
# Host C toolchain for build scripts, plus llvm-rc (icon resource) and lld-link (linker)
sudo apt-get install -y build-essential llvm clang lld

# Rust with the Windows target
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env
rustup target add x86_64-pc-windows-msvc

# cargo-xwin, which downloads the MSVC CRT and Windows SDK on first build
cargo install cargo-xwin

# Build
cargo xwin build --release
```

Output: `target/x86_64-pc-windows-msvc/release/umpv.exe`

## Acknowledgements

`umpv.ico` is a colorized version of the `video-clip` icon from Microsoft's [Fluent UI System Icons](https://github.com/microsoft/fluentui-system-icons) (MIT License).
