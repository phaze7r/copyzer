# STATE.md — Copyzer Project State

**Last Updated:** 2026-11-14  
**Version:** v0.1.0-dev  
**Phase:** Design approved → Planning & Implementation  

## Locked Architectural Decisions
- **Language:** Rust (edition 2021/2024)
- **UI:** Terminal UI (`ratatui` + `crossterm`) + CLI (`clap`)
- **iOS Backend:** CLI wrappers around `libimobiledevice` (`idevice_id`, `ideviceinfo`, `idevicepair`) and `ifuse` with RAII `MountGuard`
- **Media Grouping (v0.1):**
  1. Year → Month buckets (EXIF `DateTimeOriginal` with `mtime` fallback)
  2. Raw `DCIM/` folders
  3. Best-effort iPhone Albums via read-only `PhotoData/Photos.sqlite` (`rusqlite`) with graceful fallback
- **Format Conversion:** Deferred to v0.2+ (v0.1 copies originals intact)
- **Safety:** Never modify iPhone originals; atomic `.part` → rename; skip existing files with matching name & byte size

## Module Status
| Module | Status | Notes |
|--------|--------|-------|
| `.gitignore` & init docs | Done | Git initialized on `main` |
| `Cargo.toml` & `src/main.rs` | Pending | Crate scaffold |
| `src/theme.rs` | Pending | Omarchy `colors.toml` + fallback |
| `src/device.rs` | Pending | USB/trust + `ifuse` `MountGuard` |
| `src/scanner.rs` | Pending | `DCIM/` Year/Month + folder scan |
| `src/albums.rs` | Pending | Best-effort `Photos.sqlite` reader |
| `src/copier.rs` | Pending | Atomic copy + progress events |
| `src/cli.rs` | Pending | `doctor`, `scan`, `copy` |
| `src/tui/` | Pending | `ratatui` TUI + `?` help overlay |
| `pkgbuild/` & `README.md` | Pending | `PKGBUILD`, `.desktop`, MIT `LICENSE` |
