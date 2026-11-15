<div align="center">
  <img src="pkgbuild/copyzer.svg" alt="Copyzer logo" width="96" height="96" />
  <h1>Copyzer</h1>
  <p>
    <strong>Fast, keyboard-first iPhone media copier TUI &amp; CLI for Linux, macOS, and Windows.</strong><br />
    <sub>English <em>copy</em> + Pashto <em>zer</em> ("fast") &bull; Built by <a href="https://texodus.tech">Texodus</a></sub>
  </p>

  <p>
    <a href="https://github.com/phaze7r/copyzer/releases/tag/v0.3.0"><img src="https://img.shields.io/badge/release-v0.3.0-2563EB?style=flat-square" alt="Release v0.3.0" /></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-F59E0B?style=flat-square" alt="MIT License" /></a>
    <a href="https://phaze7r.github.io/copyzer/"><img src="https://img.shields.io/badge/site-GitHub_Pages-0F172A?style=flat-square" alt="Website" /></a>
    <a href="https://texodus.tech"><img src="https://img.shields.io/badge/brand-texodus.tech-1E293B?style=flat-square" alt="Texodus" /></a>
  </p>
</div>

---

**Copyzer** connects to your iPhone over `ifuse` / `libimobiledevice` (`afcclient`), groups photos and videos by **Year & Month**, **User Albums**, or raw **`DCIM/` folders**, and copies originals safely to your machine without modifying or deleting anything on the phone.

## Interface

| Years & Months View | Albums, Help & Live Copy Progress |
| :---: | :---: |
| ![Copyzer Years and Months TUI](docs/assets/tui-main.png) | ![Copyzer Albums and Progress TUI](docs/assets/tui-progress.png) |

## What's New in v0.3.0

- **User-Created Albums Fix (`#1`)** — Stages `PhotoData/Photos.sqlite` (+ `-wal` / `-shm`) locally before querying (with automatic `afcclient` fallback when FUSE blocks direct SQLite reads), filters strictly to user-created albums (`ZKIND = 2`), and verifies the active `Z_*ASSETS` join table against `ZGENERICALBUM` (`ZASSET` and `ZGENERICASSET`).
- **macOS & Windows Prebuilt Releases** — Ships native prebuilt binaries for **Linux (`x86_64`)**, **macOS (`aarch64` Apple Silicon & `x86_64`)**, and **Windows (`x86_64`)**, with `afcclient` fallback when `ifuse` is not installed.
- **1 MiB High-Throughput USB-C Copier** — Uses a reusable 1 MiB copy buffer with live **MB/s** speed reporting and instant mid-file cancellation for multi-gigabyte 4K `.MOV` videos.
- **Two-Phase Streaming Scan** — Unlocks **Years & Months** and **DCIM Folders** in the TUI immediately as soon as the `DCIM/` walk finishes, while **Albums** loads in the background.
- **Privacy & Personal-Data Safety** — Mounts inside a strict `0700` private directory (`XDG_RUNTIME_DIR`), sanitizes all destination paths, never overwrites existing files when `IMG_9999` counters roll over across `DCIM` folders (`IMG_0001_136APPLE.JPG`), and displays hardware model names (`iPhone 16 Pro`) rather than personal device names.

## Features

- **Year / Month bulk copy** — Groups media by capture year and month (`YYYY-MM`) with zero per-file re-stat overhead during copy.
- **Best-effort Albums** — Reads `PhotoData/Photos.sqlite` in strict read-only immutable mode when accessible, falling back gracefully to Year/Month and `DCIM/` views.
- **Safe, atomic copies** — Writes to `<filename>.copyzer.part`, syncs data to disk, and renames atomically; skips existing files with matching filename and byte size; preserves capture timestamps; never mutates iPhone originals.
- **Omarchy live theme** — Reads `~/.local/state/omarchy/current/theme/colors.toml` with a high-contrast slate dark fallback.
- **Clean RAII unmount** — Mounts via `ifuse` and unmounts automatically on exit, error, or cancellation (with lazy-unmount fallback).

## Prerequisites

Install the iOS USB stack (`usbmuxd` starts automatically via udev when you plug in an iPhone):

- **Arch Linux / Omarchy:**
  ```bash
  sudo pacman -S ifuse libimobiledevice usbmuxd
  ```
- **Debian / Ubuntu:**
  ```bash
  sudo apt install ifuse libimobiledevice6 libimobiledevice-utils usbmuxd
  ```
- **Fedora:**
  ```bash
  sudo dnf install ifuse libimobiledevice libimobiledevice-utils usbmuxd
  ```
- **macOS (Homebrew):**
  ```bash
  brew install libimobiledevice
  ```
- **Windows:**
  Install `libimobiledevice` (includes `idevice_id`, `ideviceinfo`, `idevicepair`, `afcclient`) via Scoop/WinGet or official builds and ensure it is in your `PATH`.

Plug in your iPhone via USB-C, unlock it, and tap **Trust** if prompted.

## Install

### Option 1: Arch Linux / Omarchy (Pacman / `yay -Syu` Repo)

Run the installer once to add the `[copyzer]` repository to `/etc/pacman.conf` and install `copyzer`:

```bash
curl -fsSL https://raw.githubusercontent.com/phaze7r/copyzer/main/install.sh | bash
```

Or add the repository manually to `/etc/pacman.conf`:

```ini
[copyzer]
SigLevel = Optional TrustAll
Server = https://github.com/phaze7r/copyzer/releases/latest/download
```

Then install and update just like any other Arch package:

```bash
sudo pacman -Sy copyzer
# Future updates happen automatically with:
sudo pacman -Syu   # or: yay -Syu
```

*(You can also build locally from source with `cd pkgbuild && makepkg -si`.)*

### Option 2: Universal Linux & macOS (`install.sh` / Prebuilt Release)

Download and install the `v0.3.0` release binary:

```bash
curl -fsSL https://raw.githubusercontent.com/phaze7r/copyzer/main/install.sh | bash
```

Pass `--user` (or run without root) to install into `~/.local/bin` instead of `/usr/local/bin`.

### Option 3: Windows (`x86_64` Prebuilt ZIP)

Download `copyzer-v0.3.0-windows-x86_64.zip` from the [v0.3.0 Releases page](https://github.com/phaze7r/copyzer/releases/tag/v0.3.0), extract `copyzer.exe`, and run it in Windows Terminal or PowerShell.

### Option 4: Cargo

```bash
cargo install --git https://github.com/phaze7r/copyzer --tag v0.3.0
```

## Connecting Your iPhone (Important)

Once you plug your iPhone into your PC or USB hub:
1. **Unlock your iPhone** with Face ID or your passcode.
2. If iOS shows **"Allow Accessory to Connect?"**, tap **Allow** (iOS blocks USB data until accessories are allowed).
3. If iOS shows **"Trust This Computer?"**, tap **Trust** and enter your iPhone passcode.
4. Launch `copyzer` (or press `r` inside Copyzer to refresh).

## Keyboard Shortcuts (TUI)

Launch the TUI by running `copyzer` (or selecting **Copyzer** from your app launcher):

| Key | Action |
| --- | --- |
| `Tab` / `1`–`3` | Switch view (`Years/Months`, `Albums`, `DCIM Folders`) |
| `Up` / `Down` or `j` / `k` | Navigate |
| `Space` | Toggle selection |
| `a` | Select all / none in current view |
| `d` | Edit destination path (default `~/Pictures/Copyzer`) |
| `r` | Refresh device & rescan |
| `Enter` / `c` | Start copy |
| `?` | Toggle help overlay |
| `q` / `Esc` | Cancel active copy or quit and unmount cleanly |

## CLI Usage

```bash
# Launch interactive TUI
copyzer

# Check usbmuxd, ifuse, libimobiledevice, and connected iPhone status
copyzer doctor

# Scan connected iPhone (or local mount) and print JSON
copyzer scan --json

# Copy media headlessly
copyzer copy --year 2026 --month 11
```

## Roadmap & Future Features (v0.3+)

- **Lightning & Older USB 2.0 iPhone Support** — Adaptive USB 2.0 handshake retries, legacy iOS 12–15 `Photos.sqlite` schema adapters (`ZGENERICASSET`), and full `ProductType` mapping back to iPhone 6s / SE.
- **Optional Format Conversion** — On-the-fly `HEIC → JPEG` and `MOV → MP4` transcoding during copy while preserving originals.
- **Incremental Backup Profiles** — Saved destination profiles and one-key "Copy Since Last Backup" mode.
- **Inline Terminal Previews** — Optional Kitty / Sixel thumbnail preview for highlighted buckets.

## Contributing & Reporting Issues

Contributions, bug reports, and hardware test reports are very welcome!

- **Found a bug or edge case?** Please [open an issue](https://github.com/phaze7r/copyzer/issues) and include the output of `copyzer doctor` along with your iPhone model, iOS version, and Linux distro.
- **Want to build a roadmap feature or fix a bug?**
  1. Fork the repository and create a feature branch.
  2. Run the test suite and linter locally:
     ```bash
     cargo test
     cargo clippy --all-targets -- -D warnings
     ```
  3. Open a Pull Request describing your changes.

## Website & Branding

- **Project Site (GitHub Pages):** [phaze7r.github.io/copyzer](https://phaze7r.github.io/copyzer/) (source in [`docs/`](docs/index.html))
- **Built by:** [Texodus](https://texodus.tech)

## License

[MIT](LICENSE)
