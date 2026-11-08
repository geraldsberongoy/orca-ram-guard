# Orca RAM & Port Guard (`orca-ram-guard`)

Keep [Orca](https://github.com/stablyai/orca) memory usage low on macOS (especially on 8 GB / 16 GB Macs) by detecting and closing idle background AI agent terminals (`claude`, `codex`, `antigravity`, orphaned PTYs) and stray localhost dev servers.

## ✨ What It Includes

1. **Orca Status-Bar Plugin (`orca-plugin.json` + `src/worker.mjs`)**
   - Displays live Orca memory footprint and reclaimable idle terminal RAM in the bottom status bar: `RAM 1.18 GB (161 MB idle)`.
   - Displays active non-system dev server ports in the status bar: `Ports: 2 (:3847, :9224)`.
   - Color-coded warnings (`warning` at ≥150 MB idle, `danger` at ≥500 MB idle) and desktop toast notifications.
   - Click either status bar item (or run from Command Palette `Cmd+Shift+P`) to open a native macOS multi-select cleanup picker.

2. **Interactive Multi-Select Idle Terminal Closer (`orca-ram-guard-check.mjs`)**
   - Combines `orca diagnostics memory --json` and `orca terminal list --json`.
   - Identifies terminals that have been idle for `>=15m` and consume `>=40 MB` (or orphaned PTYs).
   - Opens a native macOS multi-select dialog (`choose from list ... with multiple selections allowed`) so you can `⌘-Click` the exact idle terminals you want to close.

3. **Interactive Multi-Select Port & Dev-Server Killer (`orca-port-guard-check.mjs`)**
   - Reads `orca ports --all --json` and filters out system/IPC processes to list your active localhost dev servers (`node`, `vite`, `next`, `wrangler`, etc.) and their worktree owners.
   - Lets you multi-select and `SIGTERM` forgotten servers in 1 click.

4. **Zero-RAM macOS `launchd` Background Agent (`install.sh`)**
   - Runs `orca-ram-guard-check.mjs` every 20 minutes via `launchctl` with 0 MB persistent memory overhead.

---

## 🚀 Quick Install

```bash
git clone https://github.com/geraldsberongoy/orca-ram-guard.git ~/orca/orca-ram-guard
cd ~/orca/orca-ram-guard
chmod +x install.sh
./install.sh
```

---

## 🛠️ Manual CLI Usage

```bash
# Dry-run JSON report of total Orca RAM and reclaimable idle terminals
node ./orca-ram-guard-check.mjs --dry-run --min-idle-mins=15

# Force-open the native macOS multi-select terminal closer dialog
node ./orca-ram-guard-check.mjs --force-dialog --min-idle-mins=10

# Dry-run JSON report of active dev ports
node ./orca-port-guard-check.mjs --dry-run

# Open the native macOS multi-select port killer dialog
node ./orca-port-guard-check.mjs
```

---

## 💡 Best Practices for Low RAM in Orca

- **Close idle AI CLI sessions (`claude`, `codex`, `agy`):** Plain `zsh` terminals only use ~5 MB, whereas an idle Node/Bun AI agent session holds 150 MB–500+ MB per terminal.
- **Reuse your main worktree for running & confirming:** Creating extra worktrees costs 0 MB RAM when idle, but running `npm install` or dev servers across multiple worktrees duplicates `node_modules` on disk and language servers in RAM.
- **Headless Mode (`orca serve`):** When you need to save ~600 MB–1 GB of Electron GUI memory, quit the Orca desktop app and run `orca serve` from a regular terminal.
