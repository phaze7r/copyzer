# ANTIGRAVITY.md — Copyzer Agent Init Guide

> Read `STATE.md` and `HANDOFF.md` before doing any work. Update both before finishing your turn.

## Project Identity
- **Name:** Copyzer (`copyzer` — English *copy* + Pashto *zer* "fast")
- **Purpose:** Open-source Rust TUI + CLI for Arch Linux & Omarchy to connect an iPhone (16 Pro Max / USB-C) and bulk-copy media by Year/Month, Albums (best-effort), or `DCIM/` folders.
- **Root:** `/home/batcillos/Work/Github/copyzer`
- **Spec:** `docs/superpowers/specs/2026-11-14-copyzer-v0.1-design.md`

## Mandatory Skills & Engineering Bar
Operate as a top-1% Rust & Arch/Omarchy systems engineer:
1. **`ponytail`** — Smallest complete change that solves the problem. No over-engineering, no speculative abstractions.
2. **`omarchy` / `omarchy-app`** — Follow Omarchy conventions: live theme colors (`~/.local/state/omarchy/current/theme/colors.toml`), keyboard-first UX, `PKGBUILD` + `.desktop` launcher integration, never modify `/usr/share/omarchy/`.
3. **`ui-ux-pro-max`** — High-contrast, clean flat TUI hierarchy, clear status/error messaging, zero clutter.
4. **`graphify`** — Keep code modular with clean boundaries; update/query `graphify-out/` when exploring architecture.
5. **`superpowers` (`test-driven-development`, `verification-before-completion`)** — Write tests with `tempfile` fixtures for every module; run `cargo test` and `cargo clippy --all-targets -- -D warnings` before claiming done.
6. **`prompt-engineering-patterns`** — When delegating to subagents, provide exact file paths, interface signatures, constraints, and verification commands.

## Hard Rules
- **Always keep `.gitignore` up to date** — never commit `target/`, `*.part`, `pkgbuild/pkg/`, `pkgbuild/src/`, or `graphify-out/.graphify_*`.
- **Never mutate iPhone originals** — read-only mount/queries; atomic `.part` → rename on destination.
- **Always unmount cleanly** — `MountGuard` `Drop` must unmount `ifuse` even on error/Ctrl-C.
- **Commit `Cargo.lock`** for reproducible `PKGBUILD --locked` builds on Arch Linux.
