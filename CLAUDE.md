# CLAUDE.md — Copyzer Agent Init Guide

> Read `STATE.md` and `HANDOFF.md` before starting any task. Update `STATE.md` and `HANDOFF.md` when your task completes.

This file mirrors `ANTIGRAVITY.md` so any agent (Claude Code, Antigravity, or subagents) follows the exact same project contract.

## Project
- **Copyzer (`copyzer`)** v0.1 — Fast Rust TUI + CLI iPhone media copier for Arch Linux & Omarchy.
- **Directory:** `/home/batcillos/Work/Github/copyzer`
- **Design Spec:** `docs/superpowers/specs/2026-11-14-copyzer-v0.1-design.md`

## Commands
```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
```

## Architecture
- `src/device.rs` — `idevice_id`, `ideviceinfo`, `idevicepair`, `ifuse` + `MountGuard`
- `src/scanner.rs` — `DCIM/` walk, EXIF/mtime date bucketing (Year/Month + DCIM folders)
- `src/albums.rs` — Best-effort read-only `PhotoData/Photos.sqlite` album mapping
- `src/copier.rs` — Atomic `.part` → rename copy, duplicate skip (name + size)
- `src/theme.rs` — Live `~/.local/state/omarchy/current/theme/colors.toml` parser
- `src/cli.rs` — `clap` CLI (`doctor`, `scan`, `copy`, `--mount-dir`)
- `src/tui/` — `ratatui` + `crossterm` keyboard-first TUI
- `pkgbuild/` — Arch `PKGBUILD` & `copyzer.desktop`

## Rules
- Follow `ponytail` (minimal, clean, no bloat), `omarchy`, `ui-ux-pro-max`, `graphify`, `superpowers`, and `prompt-engineering-patterns`.
- Never commit `target/`, `*.part`, or `pkgbuild/{pkg,src}/`.
- Never modify source files on the iPhone; copy atomically.
