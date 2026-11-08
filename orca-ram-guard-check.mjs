#!/usr/bin/env node
/**
 * Orca RAM Guard — macOS Idle Terminal & Memory Guard (Multi-Select Picker)
 *
 * Inspects `orca diagnostics memory --json` and `orca terminal list --json`.
 * Finds heavy or idle background terminals, presents a native macOS multi-select
 * list (`choose from list ... with multiple selections allowed`), and closes ONLY
 * the specific terminals the user selected.
 *
 * Usage:
 *   node orca-ram-guard-check.mjs [--force-dialog] [--min-idle-mins=15] [--min-heavy-mb=40] [--min-total-idle-mb=150] [--dry-run]
 */

import { execFileSync } from 'node:child_process';

const ORCA_BIN = process.env.ORCA_CLI_COMMAND || '/Applications/Orca.app/Contents/Resources/bin/orca';

const args = process.argv.slice(2);
const forceDialog = args.includes('--force-dialog');
const dryRun = args.includes('--dry-run');
const minIdleMins = Number(args.find((a) => a.startsWith('--min-idle-mins='))?.split('=')[1] ?? 15);
const minHeavyMb = Number(args.find((a) => a.startsWith('--min-heavy-mb='))?.split('=')[1] ?? 40);
const minTotalIdleMb = Number(args.find((a) => a.startsWith('--min-total-idle-mb='))?.split('=')[1] ?? 100);

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

function formatGb(bytes) {
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatIdle(mins) {
  if (mins >= 999) return 'orphaned/idle';
  if (mins >= 60) {
    const h = Math.floor(mins / 60);
    const m = mins % 60;
    return `idle ${h}h ${m}m`;
  }
  return `idle ${mins}m`;
}

function main() {
  const memResp = runOrcaJson(['diagnostics', 'memory']);
  const termResp = runOrcaJson(['terminal', 'list']);
  if (!memResp?.ok || !termResp?.ok) {
    process.exit(0);
  }

  const memResult = memResp.result;
  const terminals = termResp.result.terminals || [];

  const sessionMemMap = new Map();
  const worktreeNameMap = new Map();
  for (const wt of memResult.worktrees || []) {
    worktreeNameMap.set(wt.worktreeId, wt.worktreeName || wt.repoName || wt.worktreeId);
    for (const s of wt.sessions || []) {
      sessionMemMap.set(s.sessionId, s.memory || 0);
    }
  }

  const now = Date.now();
  const candidates = [];

  for (const t of terminals) {
    const memBytes = sessionMemMap.get(t.ptyId) || 0;
    const memMb = memBytes / (1024 * 1024);
    const idleMs = t.lastOutputAt ? Math.max(0, now - t.lastOutputAt) : Infinity;
    const idleMins = idleMs === Infinity ? 999 : Math.round(idleMs / 60000);
    const wtName = worktreeNameMap.get(t.worktreeId) || t.worktreePath?.split('/').pop() || 'unknown';

    // Never auto-suggest the terminal that had output in the last 1 minute unless orphaned
    if (!t.orphaned && idleMins < 1) {
      continue;
    }

    const isHeavyIdle = memMb >= minHeavyMb && idleMins >= minIdleMins;
    const isOrphaned = Boolean(t.orphaned);
    const isLongIdleShell = idleMins >= 45 && memMb >= 4;
    const isForceCandidate = forceDialog && (memMb >= 20 || idleMins >= 5 || isOrphaned);

    if (isHeavyIdle || isOrphaned || isLongIdleShell || isForceCandidate) {
      candidates.push({
        handle: t.handle,
        worktreeName: wtName,
        title: (t.title || (t.orphaned ? 'Orphaned PTY' : 'Terminal')).replace(/[\r\n]+/g, ' '),
        agent: t.agentIdentity || 'shell',
        memBytes,
        memMb: Math.round(memMb),
        idleMins,
        orphaned: isOrphaned
      });
    }
  }

  candidates.sort((a, b) => b.memBytes - a.memBytes);
  const totalCandidateBytes = candidates.reduce((acc, c) => acc + c.memBytes, 0);
  const totalCandidateMb = Math.round(totalCandidateBytes / (1024 * 1024));

  if (dryRun) {
    console.log(
      JSON.stringify(
        {
          totalOrcaMemoryGb: formatGb(memResult.totalMemory || 0),
          candidateCount: candidates.length,
          reclaimableMb: totalCandidateMb,
          candidates
        },
        null,
        2
      )
    );
    return;
  }

  if (!forceDialog && (candidates.length === 0 || totalCandidateMb < minTotalIdleMb)) {
    return;
  }

  if (candidates.length === 0) {
    execFileSync('osascript', [
      '-e',
      `display notification "Total Orca RAM: ${formatGb(memResult.totalMemory || 0)}. No idle terminals found!" with title "Orca RAM Guard"`
    ]);
    return;
  }

  // Build unique display labels mapped to candidate handles
  const labelToCandidate = new Map();
  const listItems = [];
  const defaultItems = [];

  candidates.forEach((c, idx) => {
    const label = `${idx + 1}. [${c.memMb} MB] ${c.worktreeName} — ${c.agent} (${c.title}) • ${formatIdle(c.idleMins)}`;
    labelToCandidate.set(label, c);
    listItems.push(label);
    // Pre-select heavy idle terminals (>=40MB and >=15m idle) or orphaned terminals
    if ((c.memMb >= minHeavyMb && c.idleMins >= minIdleMins) || c.orphaned) {
      defaultItems.push(label);
    }
  });

  const promptText =
    `Orca Total RAM: ${formatGb(memResult.totalMemory || 0)} (${totalCandidateMb} MB in ${candidates.length} idle terminals)\n` +
    `Select which terminals to close (⌘-Click or Shift-Click to select multiple):`;

  const appleScript = `
    set itemList to ${JSON.stringify(listItems)}
    set defaultList to ${JSON.stringify(defaultItems.length > 0 ? defaultItems : listItems.slice(0, 1))}
    tell application "System Events"
      activate
      set picked to choose from list itemList with title "Orca RAM Guard — Choose Terminals to Close" with prompt ${JSON.stringify(promptText)} default items defaultList OK button name "Close Selected" cancel button name "Keep All" with multiple selections allowed
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
    let closedCount = 0;
    let freedMb = 0;

    for (const label of selectedLabels) {
      const target = labelToCandidate.get(label);
      if (!target) continue;
      try {
        execFileSync(ORCA_BIN, ['terminal', 'close', '--terminal', target.handle, '--json'], {
          stdio: 'ignore',
          timeout: 5000
        });
        closedCount++;
        freedMb += target.memMb;
      } catch {
        // ignore individual close errors
      }
    }

    if (closedCount > 0) {
      execFileSync('osascript', [
        '-e',
        `display notification "Closed ${closedCount} selected terminal(s) and freed ~${freedMb} MB RAM." with title "Orca RAM Guard"`
      ]);
    }
  } catch {
    // User cancelled or closed window
  }
}

main();
