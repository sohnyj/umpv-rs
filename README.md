# umpv-rs

A single-instance mpv launcher for Windows, written in Rust. Based on the [umpv](https://github.com/mpv-player/mpv/blob/master/TOOLS/umpv) Python script from the mpv project. Opens files in a running mpv window via named pipe IPC, or launches a new instance if none is running.

## Minimum Requirements

- OS: Windows 10+ (x64)
- CPU: x86_64-v3 (AVX2 support required)

## Usage

**Place umpv.exe in the same directory as mpv.exe.** umpv launches `mpv.exe` from its own directory, so PATH is not consulted.

### 1. Register file associations with mpv ([mpv's `--register`](https://mpv.io/manual/master/#options-register))

```bat
.\mpv.com --register --video-exts=mp4,mkv --audio-exts= --image-exts= --archive-exts= --playlist-exts=
```

Specify the extensions you want. Leave a category empty (`=`) to skip it.

**umpv does not support system-wide associations.**

### 2. Add umpv to mpv's registered extensions

Only processes extensions that were registered by mpv's `--register` (step 1).

```bat
.\umpv.exe --register
```

> [!NOTE]
> umpv only supports per-user file associations (`HKEY_CURRENT_USER`). **Running as administrator is neither required nor supported, and umpv does not support system-wide associations.**
> To set umpv as the default for each extension, go to Windows Settings > Apps > Default apps > mpv, and select umpv for the desired extensions.

`--loadfile=` is optional; if omitted, defaults to `replace`. `--idlescreen=` is optional; if omitted, defaults to `no`. Example:

```bat
.\umpv.exe --register --loadfile=append+play --idlescreen=yes
```

### 3. Unregister umpv

```bat
.\umpv.exe --unregister
```

Removes umpv file associations from the registry. Does not restore previous defaults.

## Loadfile modes

The `--loadfile=<value>` option controls how files are added to the mpv playlist. The mode is specified at registration time and baked into the registered command line.

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

See the [mpv documentation](https://mpv.io/manual/master/#command-interface-[%3Coptions%3E]]]) for the full list of options.

## Idlescreen

The `--idlescreen=<value>` option controls whether mpv shows its logo and idle message while waiting for a file. The value is specified at registration time and baked into the registered command line.

| Value | Description |
|-------|-------------|
| `no` | Do not show the idle logo (default) |
| `yes` | Show the idle logo |

Setting `no` avoids a brief idle-logo flash when umpv launches a new mpv instance. Applied only on launch, not when sending files to a running instance. Sets the OSC script option [`idlescreen`](https://mpv.io/manual/master/#on-screen-controller-idlescreen) via `--script-opts=osc-idlescreen=`, so it requires mpv's built-in OSC (not `--no-osc`).

## Cross-compiling

umpv is cross-compiled from Linux (including WSL) to the `x86_64-pc-windows-msvc` target. CI builds use the same toolchain ([build.yml](.github/workflows/build.yml)).

### Requirements

- [Rust](https://www.rust-lang.org/) with the `x86_64-pc-windows-msvc` target
- [cargo-xwin](https://github.com/rust-cross/cargo-xwin): downloads the MSVC CRT and Windows SDK automatically
- [LLVM](https://llvm.org/) tools:
  - `llvm-rc`: compiles the icon resource; requires `clang` for preprocessing
  - `lld-link`: linker for the MSVC target
- A host C toolchain (`cc`): required to compile build scripts and cargo-xwin itself

### Setup (Ubuntu / WSL)

```bash
# Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env
rustup target add x86_64-pc-windows-msvc

# Host C toolchain and LLVM tools
sudo apt-get install -y build-essential llvm clang lld

# cargo-xwin
cargo install cargo-xwin
```

### Build

```bash
cargo xwin build --release
```

Output: `target/x86_64-pc-windows-msvc/release/umpv.exe`

## Acknowledgements

`mpv-icon.ico` is property of the [mpv project](https://github.com/mpv-player/mpv).
