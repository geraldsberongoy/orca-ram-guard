import { execFile } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const PLUGIN_ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const CHECK_SCRIPT = join(PLUGIN_ROOT, 'orca-ram-guard-check.mjs');
const PORT_SCRIPT = join(PLUGIN_ROOT, 'orca-port-guard-check.mjs');

const POLL_INTERVAL_MS = 60 * 1000; // Update status bar every 60s
const ALERT_COOLDOWN_MS = 20 * 60 * 1000; // Auto-notify at most once every 20 mins
const ALERT_RECLAIMABLE_MB = 300; // Notify when >=300 MB is sitting in idle terminals

let pollTimer = null;
let lastAlertAt = 0;

async function scanMemory() {
  try {
    const { stdout } = await execFileAsync(
      process.execPath,
      [CHECK_SCRIPT, '--dry-run', '--min-idle-mins=15', '--min-heavy-mb=40'],
      { timeout: 10000 }
    );
    return JSON.parse(stdout);
  } catch {
    return null;
  }
}

async function scanPorts() {
  try {
    const { stdout } = await execFileAsync(
      process.execPath,
      [PORT_SCRIPT, '--dry-run'],
      { timeout: 10000 }
    );
    return JSON.parse(stdout);
  } catch {
    return null;
  }
}

export default function activate(orca) {
  async function refreshStatusBar() {
    const [report, portReport] = await Promise.all([scanMemory(), scanPorts()]);

    if (report) {
      const { totalOrcaMemoryGb, candidateCount, reclaimableMb, candidates } = report;
      const hasHeavyIdle = candidateCount > 0 && reclaimableMb >= 100;

      const text = hasHeavyIdle
        ? `RAM ${totalOrcaMemoryGb} (${reclaimableMb} MB idle)`
        : `RAM ${totalOrcaMemoryGb}`;

      const tone =
        reclaimableMb >= 500 ? 'danger' : reclaimableMb >= 150 ? 'warning' : 'neutral';

      const topDetails = (candidates || [])
        .slice(0, 4)
        .map((c) => `${c.worktreeName} (${c.agent}: ${c.memMb}MB, ${c.idleMins}m idle)`)
        .join(' | ');

      const tooltip = hasHeavyIdle
        ? `Click to choose & close idle terminals (~${reclaimableMb} MB across ${candidateCount} idle): ${topDetails}`
        : `Orca total footprint: ${totalOrcaMemoryGb}. Click to check idle terminals.`;

      try {
        await orca.host.call('statusBar.update', {
          id: 'ram-status',
          text,
          tone,
          icon: 'gauge',
          tooltip,
          visible: true
        });
      } catch (err) {
        orca.log(`statusBar.update (ram) error: ${err?.message || err}`);
      }

      const now = Date.now();
      if (reclaimableMb >= ALERT_RECLAIMABLE_MB && now - lastAlertAt >= ALERT_COOLDOWN_MS) {
        lastAlertAt = now;
        try {
          await orca.host.call('notifications.show', {
            title: `Orca RAM Guard: ${reclaimableMb} MB in ${candidateCount} idle terminals`,
            body: `Click "RAM ${totalOrcaMemoryGb}" in the Orca status bar to pick which idle agents to close (${topDetails}).`
          });
        } catch {
          // ignore notification error
        }
      }
    }

    if (portReport) {
      const { activePortCount, ports } = portReport;
      const portListStr = (ports || [])
        .slice(0, 4)
        .map((p) => `:${p.port} (${p.processName} in ${p.worktreeName})`)
        .join(' | ');

      const portText =
        activePortCount > 0
          ? `Ports: ${activePortCount} (${ports.slice(0, 2).map((p) => `:${p.port}`).join(', ')})`
          : 'Ports: 0';

      const portTone = activePortCount >= 3 ? 'warning' : 'neutral';
      const portTooltip =
        activePortCount > 0
          ? `Click to choose & stop active ports: ${portListStr}`
          : 'No active dev servers or listening user ports detected.';

      try {
        await orca.host.call('statusBar.update', {
          id: 'port-status',
          text: portText,
          tone: portTone,
          icon: 'plug',
          tooltip: portTooltip,
          visible: true
        });
      } catch (err) {
        orca.log(`statusBar.update (ports) error: ${err?.message || err}`);
      }
    }
  }

  orca.commands.register('ramGuard.promptCleanup', async () => {
    try {
      await execFileAsync(
        process.execPath,
        [CHECK_SCRIPT, '--force-dialog', '--min-idle-mins=10', '--min-heavy-mb=40'],
        { timeout: 65000 }
      );
    } catch {
      // dialog timed out or cancelled
    }
    await refreshStatusBar();
  });

  orca.commands.register('ramGuard.promptPortCleanup', async () => {
    try {
      await execFileAsync(process.execPath, [PORT_SCRIPT], { timeout: 65000 });
    } catch {
      // dialog timed out or cancelled
    }
    await refreshStatusBar();
  });

  void refreshStatusBar();
  pollTimer = setInterval(() => {
    void refreshStatusBar();
  }, POLL_INTERVAL_MS);
}

export function deactivate() {
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}
