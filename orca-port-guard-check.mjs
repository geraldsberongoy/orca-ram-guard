#!/usr/bin/env node
/**
 * Orca Port & Dev-Server Guard — macOS Multi-Select Port Killer
 *
 * Inspects `orca ports --all --json` and filters out system/Orca/Antigravity internal ports
 * so you only see dev servers and listening user processes (e.g., node, next, vite, python, wrangler).
 * Shows a native macOS multi-select picker (`choose from list ... with multiple selections allowed`)
 * and terminates the selected server PIDs.
 *
 * Usage:
 *   node orca-port-guard-check.mjs [--dry-run]
 */

import { execFileSync } from 'node:child_process';

const ORCA_BIN = process.env.ORCA_CLI_COMMAND || '/Applications/Orca.app/Contents/Resources/bin/orca';

const args = process.argv.slice(2);
const dryRun = args.includes('--dry-run');

// Ignore system daemons and Orca/Antigravity internal IPC ports
const IGNORED_PROCESSES = new Set([
  'Orca',
  'ControlCenter',
  'rapportd',
  'sharingd',
  'mDNSResponder',
  'adb',
  'agy',
  'antigravity',
  'claude',
  'codex'
]);

function runOrcaJson(subargs) {
  try {
    const out = execFileSync(ORCA_BIN, [...subargs, '--json'], {
      encoding: 'utf8',
      timeout: 10000,
      stdio: ['ignore', 'pipe', 'ignore']
    });
    return JSON.parse(out);
  } catch {
    return null;
  }
}

function getProcessRssMb(pid) {
  try {
    const out = execFileSync('ps', ['-p', String(pid), '-o', 'rss='], {
      encoding: 'utf8',
      timeout: 2000
    }).trim();
    const kb = Number(out);
    return Number.isFinite(kb) && kb > 0 ? Math.max(1, Math.round(kb / 1024)) : 0;
  } catch {
    return 0;
  }
}

function getProcessCommand(pid) {
  try {
    const out = execFileSync('ps', ['-p', String(pid), '-o', 'command='], {
      encoding: 'utf8',
      timeout: 2000
    }).trim();
    return out.slice(0, 80);
  } catch {
    return '';
  }
}

function main() {
  const portsResp = runOrcaJson(['ports', '--all']);
  if (!portsResp?.ok) {
    process.exit(0);
  }

  const rawPorts = portsResp.result?.ports || [];
  const devPorts = [];
  const seenKey = new Set();

  for (const p of rawPorts) {
    if (!p.pid || !p.port) continue;
    if (IGNORED_PROCESSES.has(p.processName)) continue;

    const key = `${p.pid}:${p.port}`;
    if (seenKey.has(key)) continue;
    seenKey.add(key);

    const rssMb = getProcessRssMb(p.pid);
    const cmd = getProcessCommand(p.pid);
    if (cmd.includes('Raycast')) continue;

    const worktreeName = p.owner?.path
      ? p.owner.path.split('/').pop()
      : p.owner?.displayName || 'external';

    devPorts.push({
      port: p.port,
      pid: p.pid,
      processName: cmd && cmd.length < 36 ? cmd : p.processName || 'process',
      worktreeName,
      rssMb,
      cmd
    });
  }

  devPorts.sort((a, b) => a.port - b.port);

  if (dryRun) {
    console.log(
      JSON.stringify(
        {
          activePortCount: devPorts.length,
          ports: devPorts
        },
        null,
        2
      )
    );
    return;
  }

  if (devPorts.length === 0) {
    execFileSync('osascript', [
      '-e',
      'display notification "No active dev servers or stray localhost ports detected." with title "Orca Port Guard"'
    ]);
    return;
  }

  const labelToPort = new Map();
  const listItems = [];

  devPorts.forEach((p) => {
    const memTag = p.rssMb > 0 ? `${p.rssMb} MB` : 'listening';
    const label = `:${p.port} — ${p.processName} (${p.worktreeName}, PID ${p.pid}, ${memTag})`;
    labelToPort.set(label, p);
    listItems.push(label);
  });

  const promptText =
    `Found ${devPorts.length} active dev server / listening port(s).\n` +
    `Select which port process(es) to stop (⌘-Click to select multiple):`;

  const appleScript = `
    set itemList to ${JSON.stringify(listItems)}
    tell application "System Events"
      activate
      set picked to choose from list itemList with title "Orca Port Guard — Active Dev Servers" with prompt ${JSON.stringify(promptText)} default items {item 1 of itemList} OK button name "Kill Selected Port(s)" cancel button name "Keep Running" with multiple selections allowed
    end tell
    if picked is false then
      return "CANCELLED"
    end if
    set AppleScript's text item delimiters to "\\n"
    return picked as text
  `;

  try {
    const output = execFileSync('osascript', ['-e', appleScript], { encoding: 'utf8' }).trim();
    if (!output || output === 'CANCELLED') {
      return;
    }

    const selectedLabels = output.split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
    let killedCount = 0;
    const killedPorts = [];

    for (const label of selectedLabels) {
      const target = labelToPort.get(label);
      if (!target) continue;
      try {
        process.kill(target.pid, 'SIGTERM');
        killedCount++;
        killedPorts.push(`:${target.port}`);
      } catch {
        // ignore if already exited
      }
    }

    if (killedCount > 0) {
      execFileSync('osascript', [
        '-e',
        `display notification "Stopped ${killedCount} server(s) on port(s) ${killedPorts.join(', ')}." with title "Orca Port Guard"`
      ]);
    }
  } catch {
    // User cancelled
  }
}

main();
