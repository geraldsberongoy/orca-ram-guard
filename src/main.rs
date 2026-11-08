use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::env;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_ORCA_BIN: &str = "/Applications/Orca.app/Contents/Resources/bin/orca";

// --- JSON Schemas from `orca` CLI ---

#[derive(Deserialize)]
struct OrcaEnvelope<T> {
    ok: Option<bool>,
    result: Option<T>,
}

#[derive(Deserialize)]
struct MemoryDiagnosticsResult {
    #[serde(rename = "totalMemory")]
    total_memory: Option<u64>,
    worktrees: Option<Vec<WorktreeMemory>>,
}

#[derive(Deserialize)]
struct WorktreeMemory {
    #[serde(rename = "worktreeId")]
    worktree_id: String,
    #[serde(rename = "worktreeName")]
    worktree_name: Option<String>,
    #[serde(rename = "repoName")]
    repo_name: Option<String>,
    sessions: Option<Vec<SessionMemory>>,
}

#[derive(Deserialize)]
struct SessionMemory {
    #[serde(rename = "sessionId")]
    session_id: String,
    memory: Option<u64>,
}

#[derive(Deserialize)]
struct TerminalListResult {
    terminals: Option<Vec<OrcaTerminal>>,
}

#[derive(Deserialize)]
struct OrcaTerminal {
    handle: String,
    #[serde(rename = "ptyId")]
    pty_id: String,
    orphaned: Option<bool>,
    #[serde(rename = "worktreeId")]
    worktree_id: String,
    #[serde(rename = "worktreePath")]
    worktree_path: Option<String>,
    title: Option<String>,
    #[serde(rename = "lastOutputAt")]
    last_output_at: Option<u64>,
    #[serde(rename = "agentIdentity")]
    agent_identity: Option<String>,
}

#[derive(Deserialize)]
struct PortsResult {
    ports: Option<Vec<OrcaPort>>,
}

#[derive(Deserialize)]
struct OrcaPort {
    port: Option<serde_json::Value>,
    pid: Option<serde_json::Value>,
    #[serde(rename = "processName")]
    process_name: Option<String>,
    owner: Option<serde_json::Value>,
}

// --- Output Schemas for Dry-Run & Plugin Worker ---

#[derive(Serialize, Clone)]
struct CandidateTerminal {
    handle: String,
    #[serde(rename = "worktreeName")]
    worktree_name: String,
    title: String,
    agent: String,
    #[serde(rename = "memBytes")]
    mem_bytes: u64,
    #[serde(rename = "memMb")]
    mem_mb: u64,
    #[serde(rename = "idleMins")]
    idle_mins: u64,
    orphaned: bool,
}

#[derive(Serialize)]
struct RamReport {
    #[serde(rename = "totalOrcaMemoryGb")]
    total_orca_memory_gb: String,
    #[serde(rename = "candidateCount")]
    candidate_count: usize,
    #[serde(rename = "reclaimableMb")]
    reclaimable_mb: u64,
    candidates: Vec<CandidateTerminal>,
}

#[derive(Serialize, Clone)]
struct CandidatePort {
    port: u32,
    pid: u32,
    #[serde(rename = "processName")]
    process_name: String,
    #[serde(rename = "worktreeName")]
    worktree_name: String,
    #[serde(rename = "rssMb")]
    rss_mb: u64,
    cmd: String,
}

#[derive(Serialize)]
struct PortReport {
    #[serde(rename = "activePortCount")]
    active_port_count: usize,
    ports: Vec<CandidatePort>,
}

fn orca_bin() -> String {
    env::var("ORCA_CLI_COMMAND").unwrap_or_else(|_| DEFAULT_ORCA_BIN.to_string())
}

fn run_orca_json(args: &[&str]) -> Option<String> {
    let mut full_args = args.to_vec();
    full_args.push("--json");
    let output = Command::new(orca_bin()).args(&full_args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn format_gb(bytes: u64) -> String {
    format!("{:.2} GB", (bytes as f64) / (1024.0 * 1024.0 * 1024.0))
}

fn format_idle(mins: u64) -> String {
    if mins >= 999 {
        "orphaned/idle".to_string()
    } else if mins >= 60 {
        format!("idle {}h {}m", mins / 60, mins % 60)
    } else {
        format!("idle {}m", mins)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn run_ram_check(force_dialog: bool, dry_run: bool, min_idle_mins: u64, min_heavy_mb: u64, min_total_idle_mb: u64) {
    let Some(mem_raw) = run_orca_json(&["diagnostics", "memory"]) else {
        return;
    };
    let Some(term_raw) = run_orca_json(&["terminal", "list"]) else {
        return;
    };

    let Ok(mem_env) = serde_json::from_str::<OrcaEnvelope<MemoryDiagnosticsResult>>(&mem_raw) else {
        return;
    };
    let Ok(term_env) = serde_json::from_str::<OrcaEnvelope<TerminalListResult>>(&term_raw) else {
        return;
    };

    if mem_env.ok != Some(true) || term_env.ok != Some(true) {
        return;
    }

    let Some(mem_result) = mem_env.result else { return };
    let Some(term_result) = term_env.result else { return };

    let mut session_mem_map: HashMap<String, u64> = HashMap::new();
    let mut worktree_name_map: HashMap<String, String> = HashMap::new();

    if let Some(worktrees) = mem_result.worktrees {
        for wt in worktrees {
            let name = wt
                .worktree_name
                .or(wt.repo_name)
                .unwrap_or_else(|| wt.worktree_id.clone());
            worktree_name_map.insert(wt.worktree_id, name);
            if let Some(sessions) = wt.sessions {
                for s in sessions {
                    session_mem_map.insert(s.session_id, s.memory.unwrap_or(0));
                }
            }
        }
    }

    let now = now_ms();
    let mut candidates: Vec<CandidateTerminal> = Vec::new();

    for t in term_result.terminals.unwrap_or_default() {
        let mem_bytes = *session_mem_map.get(&t.pty_id).unwrap_or(&0);
        let mem_mb = (mem_bytes as f64 / (1024.0 * 1024.0)).round() as u64;
        let is_orphaned = t.orphaned.unwrap_or(false);

        let idle_mins = match t.last_output_at {
            Some(ts) if now >= ts => ((now - ts) as f64 / 60000.0).round() as u64,
            Some(_) => 0,
            None => 999,
        };

        if !is_orphaned && idle_mins < 1 {
            continue;
        }

        let wt_name = worktree_name_map
            .get(&t.worktree_id)
            .cloned()
            .or_else(|| {
                t.worktree_path
                    .as_deref()
                    .and_then(|p| p.rsplit('/').next())
                    .map(String::from)
            })
            .unwrap_or_else(|| "unknown".to_string());

        let is_heavy_idle = mem_mb >= min_heavy_mb && idle_mins >= min_idle_mins;
        let is_long_idle_shell = idle_mins >= 45 && mem_mb >= 4;
        let is_force_candidate = force_dialog && (mem_mb >= 20 || idle_mins >= 5 || is_orphaned);

        if is_heavy_idle || is_orphaned || is_long_idle_shell || is_force_candidate {
            let raw_title = t.title.unwrap_or_else(|| {
                if is_orphaned {
                    "Orphaned PTY".to_string()
                } else {
                    "Terminal".to_string()
                }
            });
            let clean_title = raw_title.replace(['\r', '\n'], " ");
            candidates.push(CandidateTerminal {
                handle: t.handle,
                worktree_name: wt_name,
                title: clean_title,
                agent: t.agent_identity.unwrap_or_else(|| "shell".to_string()),
                mem_bytes,
                mem_mb,
                idle_mins,
                orphaned: is_orphaned,
            });
        }
    }

    candidates.sort_by(|a, b| b.mem_bytes.cmp(&a.mem_bytes));
    let total_candidate_bytes: u64 = candidates.iter().map(|c| c.mem_bytes).sum();
    let total_candidate_mb = (total_candidate_bytes as f64 / (1024.0 * 1024.0)).round() as u64;
    let total_orca_gb = format_gb(mem_result.total_memory.unwrap_or(0));

    if dry_run {
        let report = RamReport {
            total_orca_memory_gb: total_orca_gb,
            candidate_count: candidates.len(),
            reclaimable_mb: total_candidate_mb,
            candidates,
        };
        if let Ok(json) = serde_json::to_string_pretty(&report) {
            println!("{json}");
        }
        return;
    }

    if !force_dialog && (candidates.is_empty() || total_candidate_mb < min_total_idle_mb) {
        return;
    }

    if candidates.is_empty() {
        let _ = Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "display notification \"Total Orca RAM: {}. No idle terminals found!\" with title \"Orca RAM Guard (Rust)\"",
                    total_orca_gb
                ),
            ])
            .status();
        return;
    }

    let mut label_to_candidate: HashMap<String, CandidateTerminal> = HashMap::new();
    let mut list_items: Vec<String> = Vec::new();
    let mut default_items: Vec<String> = Vec::new();

    for (idx, c) in candidates.iter().enumerate() {
        let label = format!(
            "{}. [{} MB] {} — {} ({}) • {}",
            idx + 1,
            c.mem_mb,
            c.worktree_name,
            c.agent,
            c.title,
            format_idle(c.idle_mins)
        );
        if (c.mem_mb >= min_heavy_mb && c.idle_mins >= min_idle_mins) || c.orphaned {
            default_items.push(label.clone());
        }
        list_items.push(label.clone());
        label_to_candidate.insert(label, c.clone());
    }

    if default_items.is_empty() && !list_items.is_empty() {
        default_items.push(list_items[0].clone());
    }

    let prompt_text = format!(
        "Orca Total RAM: {} ({} MB in {} idle terminals)\nSelect which terminals to close (⌘-Click or Shift-Click to select multiple):",
        total_orca_gb,
        total_candidate_mb,
        candidates.len()
    );

    let items_json = serde_json::to_string(&list_items).unwrap_or_else(|_| "[]".to_string());
    let defaults_json = serde_json::to_string(&default_items).unwrap_or_else(|_| "[]".to_string());
    let prompt_json = serde_json::to_string(&prompt_text).unwrap_or_else(|_| "\"\"".to_string());

    let apple_script = format!(
        r#"
        set itemList to {items_json}
        set defaultList to {defaults_json}
        tell application "System Events"
          activate
          set picked to choose from list itemList with title "Orca RAM Guard (Rust) — Choose Terminals to Close" with prompt {prompt_json} default items defaultList OK button name "Close Selected" cancel button name "Keep All" with multiple selections allowed
        end tell
        if picked is false then
          return "CANCELLED"
        end if
        set AppleScript's text item delimiters to "\n"
        return picked as text
        "#
    );

    let Ok(out) = Command::new("osascript").args(["-e", &apple_script]).output() else {
        return;
    };
    let output_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if output_str.is_empty() || output_str == "CANCELLED" {
        return;
    }

    let mut closed_count = 0usize;
    let mut freed_mb = 0u64;

    for line in output_str.lines() {
        let trimmed = line.trim();
        if let Some(target) = label_to_candidate.get(trimmed) {
            let status = Command::new(orca_bin())
                .args(["terminal", "close", "--terminal", &target.handle, "--json"])
                .status();
            if status.map(|s| s.success()).unwrap_or(false) {
                closed_count += 1;
                freed_mb += target.mem_mb;
            }
        }
    }

    if closed_count > 0 {
        let _ = Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "display notification \"Closed {} selected terminal(s) and freed ~{} MB RAM.\" with title \"Orca RAM Guard (Rust)\"",
                    closed_count, freed_mb
                ),
            ])
            .status();
    }
}

fn get_process_rss_mb(pid: u32) -> u64 {
    let Ok(out) = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "rss="])
        .output()
    else {
        return 0;
    };
    let kb: u64 = String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    if kb > 0 {
        ((kb as f64) / 1024.0).round().max(1.0) as u64
    } else {
        0
    }
}

fn get_process_cmd(pid: u32) -> String {
    let Ok(out) = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
    else {
        return String::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .chars()
        .take(80)
        .collect()
}

fn run_port_check(dry_run: bool) {
    let Some(ports_raw) = run_orca_json(&["ports", "--all"]) else {
        return;
    };
    let Ok(ports_env) = serde_json::from_str::<OrcaEnvelope<PortsResult>>(&ports_raw) else {
        return;
    };
    if ports_env.ok != Some(true) {
        return;
    }

    let ignored: HashSet<&str> = [
        "Orca",
        "ControlCenter",
        "rapportd",
        "sharingd",
        "mDNSResponder",
        "adb",
        "agy",
        "antigravity",
        "claude",
        "codex",
    ]
    .into_iter()
    .collect();

    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    let mut dev_ports: Vec<CandidatePort> = Vec::new();

    for p in ports_env.result.and_then(|r| r.ports).unwrap_or_default() {
        let Some(port) = p.port.as_ref().and_then(|v| v.as_u64()).map(|n| n as u32) else {
            continue;
        };
        let Some(pid) = p.pid.as_ref().and_then(|v| v.as_u64()).map(|n| n as u32) else {
            continue;
        };
        let proc_name = p.process_name.unwrap_or_else(|| "process".to_string());
        if ignored.contains(proc_name.as_str()) {
            continue;
        }
        if !seen.insert((pid, port)) {
            continue;
        }

        let cmd = get_process_cmd(pid);
        if cmd.contains("Raycast") {
            continue;
        }
        let rss_mb = get_process_rss_mb(pid);
        let worktree_name = p
            .owner
            .as_ref()
            .and_then(|o| {
                o.get("path")
                    .and_then(|v| v.as_str())
                    .and_then(|path| path.rsplit('/').next().map(String::from))
                    .or_else(|| o.get("displayName").and_then(|v| v.as_str()).map(String::from))
            })
            .unwrap_or_else(|| "external".to_string());

        let display_proc = if !cmd.is_empty() && cmd.len() < 36 {
            cmd.clone()
        } else {
            proc_name
        };

        dev_ports.push(CandidatePort {
            port,
            pid,
            process_name: display_proc,
            worktree_name,
            rss_mb,
            cmd,
        });
    }

    dev_ports.sort_by_key(|p| p.port);

    if dry_run {
        let report = PortReport {
            active_port_count: dev_ports.len(),
            ports: dev_ports,
        };
        if let Ok(json) = serde_json::to_string_pretty(&report) {
            println!("{json}");
        }
        return;
    }

    if dev_ports.is_empty() {
        let _ = Command::new("osascript")
            .args([
                "-e",
                "display notification \"No active dev servers or stray localhost ports detected.\" with title \"Orca Port Guard (Rust)\"",
            ])
            .status();
        return;
    }

    let mut label_to_port: HashMap<String, CandidatePort> = HashMap::new();
    let mut list_items: Vec<String> = Vec::new();

    for p in &dev_ports {
        let mem_tag = if p.rss_mb > 0 {
            format!("{} MB", p.rss_mb)
        } else {
            "listening".to_string()
        };
        let label = format!(
            ":{} — {} ({}, PID {}, {})",
            p.port, p.process_name, p.worktree_name, p.pid, mem_tag
        );
        list_items.push(label.clone());
        label_to_port.insert(label, p.clone());
    }

    let prompt_text = format!(
        "Found {} active dev server / listening port(s).\nSelect which port process(es) to stop (⌘-Click to select multiple):",
        dev_ports.len()
    );

    let items_json = serde_json::to_string(&list_items).unwrap_or_else(|_| "[]".to_string());
    let prompt_json = serde_json::to_string(&prompt_text).unwrap_or_else(|_| "\"\"".to_string());

    let apple_script = format!(
        r#"
        set itemList to {items_json}
        tell application "System Events"
          activate
          set picked to choose from list itemList with title "Orca Port Guard (Rust) — Active Dev Servers" with prompt {prompt_json} default items {{item 1 of itemList}} OK button name "Kill Selected Port(s)" cancel button name "Keep Running" with multiple selections allowed
        end tell
        if picked is false then
          return "CANCELLED"
        end if
        set AppleScript's text item delimiters to "\n"
        return picked as text
        "#
    );

    let Ok(out) = Command::new("osascript").args(["-e", &apple_script]).output() else {
        return;
    };
    let output_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if output_str.is_empty() || output_str == "CANCELLED" {
        return;
    }

    let mut killed_ports: Vec<String> = Vec::new();
    for line in output_str.lines() {
        if let Some(target) = label_to_port.get(line.trim()) {
            let status = Command::new("kill")
                .args(["-TERM", &target.pid.to_string()])
                .status();
            if status.map(|s| s.success()).unwrap_or(false) {
                killed_ports.push(format!(":{}", target.port));
            }
        }
    }

    if !killed_ports.is_empty() {
        let _ = Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "display notification \"Stopped {} server(s) on port(s) {}.\" with title \"Orca Port Guard (Rust)\"",
                    killed_ports.len(),
                    killed_ports.join(", ")
                ),
            ])
            .status();
    }
}

fn parse_arg_u64(args: &[String], prefix: &str, default: u64) -> u64 {
    args.iter()
        .find_map(|a| a.strip_prefix(prefix))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mode_ports = args.iter().any(|a| a == "ports" || a == "--ports");
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let force_dialog = args.iter().any(|a| a == "--force-dialog");

    if mode_ports {
        run_port_check(dry_run);
    } else {
        let min_idle_mins = parse_arg_u64(&args, "--min-idle-mins=", 15);
        let min_heavy_mb = parse_arg_u64(&args, "--min-heavy-mb=", 40);
        let min_total_idle_mb = parse_arg_u64(&args, "--min-total-idle-mb=", 100);
        run_ram_check(
            force_dialog,
            dry_run,
            min_idle_mins,
            min_heavy_mb,
            min_total_idle_mb,
        );
    }
}
