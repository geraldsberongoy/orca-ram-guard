# Orca RAM & Port Guard (`orca-ram-guard`) 🦀

Ultra-light **native Rust binary** (`411 KB`, `<2 ms` startup, `~1.8 MB` RAM) + Orca status-bar plugin to keep [Orca](https://github.com/stablyai/orca) memory usage low on macOS by detecting and closing idle background AI agent terminals (`claude`, `codex`, `antigravity`, orphaned PTYs) and stray localhost dev servers.

## ✨ What It Includes

1. **Native Rust CLI & Daemon Engine (`src/main.rs` → `bin/orca-ram-guard`)**
   - Compiled with `opt-level = "z"`, `lto = true`, `strip = true`, and `panic = "abort"` into a single **411 KB** Mach-O binary.
   - Zero Node.js runtime needed for the `launchd` background daemon (`~1.8 MB` RSS instead of `~55 MB` for Node V8).
   - Combines `orca diagnostics memory --json`, `orca terminal list --json`, and `orca ports --all --json`.

2. **Interactive Multi-Select Idle Terminal Closer (`orca-ram-guard --force-dialog`)**
   - Identifies terminals that have been idle for `>=15m` and consume `>=40 MB` (or orphaned PTYs).
   - Opens a native macOS multi-select dialog (`choose from list ... with multiple selections allowed`) so you can `⌘-Click` the exact idle terminals you want to close.

3. **Interactive Multi-Select Port & Dev-Server Killer (`orca-ram-guard ports`)**
   - Filters out system/IPC processes to list your active localhost dev servers (`node`, `vite`, `next`, `wrangler`, `omp`, etc.) and their worktree owners.
   - Lets you multi-select and `SIGTERM` forgotten servers in 1 click.

4. **Orca Status-Bar Plugin (`orca-plugin.json` + `src/worker.mjs`)**
   - Displays live Orca memory footprint and reclaimable idle terminal RAM in the bottom status bar: `RAM 1.18 GB (161 MB idle)`.
   - Displays active non-system dev server ports in the status bar: `Ports: 2 (:3847, :9224)`.
   - Click either status bar item (or run from Command Palette `Cmd+Shift+P`) to launch the native Rust multi-select picker.

---

## 🚀 Quick Install

```bash
git clone https://github.com/geraldsberongoy/orca-ram-guard.git ~/orca/orca-ram-guard
cd ~/orca/orca-ram-guard
chmod +x install.sh
./install.sh
```

---

## 🛠️ Manual CLI Usage (Native Rust Binary)

```bash
# Dry-run JSON report of total Orca RAM and reclaimable idle terminals
./bin/orca-ram-guard --dry-run --min-idle-mins=15

# Force-open the native macOS multi-select terminal closer dialog
./bin/orca-ram-guard --force-dialog --min-idle-mins=10

# Dry-run JSON report of active dev ports
./bin/orca-ram-guard ports --dry-run

# Open the native macOS multi-select port killer dialog
./bin/orca-ram-guard ports
```
