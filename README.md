# Copyzer

**Copyzer** (English *copy* + Pashto *zer* "fast") is a fast, keyboard-first Rust TUI and CLI for copying photos and videos from an iPhone to Arch Linux and [Omarchy](https://omarchy.org).

## Features

- **Year / Month bulk copy** — Automatically groups `DCIM/` media by capture date (EXIF `DateTimeOriginal` with file `mtime` fallback).
- **Best-effort Albums** — Reads `PhotoData/Photos.sqlite` read-only when accessible, falling back gracefully to Year/Month and `DCIM/` folders.
- **Raw `DCIM/` folders** — Browse and copy raw camera roll folders directly.
- **Safe, atomic copies** — Writes to `<filename>.part`, syncs, and renames atomically; skips existing files with matching filename and byte size; preserves timestamps; never mutates iPhone originals.
- **Omarchy live theme** — Reads `~/.local/state/omarchy/current/theme/colors.toml` with a high-contrast dark fallback.
- **Clean unmount** — Mounts via `ifuse` and unmounts automatically on exit or `Ctrl-C`.

## Prerequisites (Arch Linux / Omarchy)

Install the required iOS USB stack and enable `usbmuxd`:

```bash
sudo pacman -S ifuse libimobiledevice usbmuxd
sudo systemctl enable --now usbmuxd
```

Plug in your iPhone via USB, unlock it, and tap **Trust** if prompted.

## Install

Build and install the Arch package locally with `makepkg`:

```bash
cd pkgbuild
makepkg -si
```

This installs `/usr/bin/copyzer` and `/usr/share/applications/copyzer.desktop` (available in Omarchy's `Super + Space` app launcher).

## Keyboard Shortcuts (TUI)

Launch the TUI by running `copyzer` (or selecting **Copyzer** from the launcher):

| Key | Action |
| --- | --- |
| `Tab` / `1`-`3` | Switch view (`Years/Months`, `Albums`, `DCIM Folders`) |
| `Up` / `Down` or `j` / `k` | Navigate |
| `Space` | Toggle selection |
| `a` | Select all / none in current view |
| `d` | Edit destination path (default `~/Pictures/Copyzer`) |
| `Enter` / `c` | Start copy |
| `?` | Toggle help overlay |
| `q` / `Esc` | Quit and unmount cleanly |

## CLI Usage

```bash
# Launch interactive TUI
copyzer

# Check usbmuxd, ifuse, libimobiledevice, and connected iPhone status
copyzer doctor

# Scan connected iPhone (or local mount) and list Year/Month, Album, and DCIM counts
copyzer scan

# Copy media headlessly
copyzer copy
```

## License

[MIT](LICENSE)
