# STATE.md — Copyzer Project State

**Last Updated:** 2026-11-14  
**Version:** v0.1.0  
**Phase:** v0.1 Implemented & Verified  

## Locked Architectural Decisions
- **Language:** Rust (edition 2021)
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
| `Cargo.toml` & `src/lib.rs` | Done | Crate & `Cargo.lock` ready |
| `src/theme.rs` | Done | Omarchy `colors.toml` + fallback (3 tests) |
| `src/device.rs` | Done | USB/trust + `ifuse` `MountGuard` (4 tests) |
| `src/scanner.rs` | Done | `DCIM/` Year/Month + folder scan (3 tests) |
| `src/albums.rs` | Done | Best-effort `Photos.sqlite` reader (3 tests) |
| `src/copier.rs` | Done | Atomic copy + progress events (4 tests) |
| `src/cli.rs` | Done | `doctor`, `scan`, `copy`, `--mount-dir` (3 tests) |
| `src/tui/` | Done | `ratatui` TUI + `?` help overlay (4 tests) |
| `pkgbuild/` & `README.md` | Done | `PKGBUILD`, `copyzer.desktop`, MIT `LICENSE` |
