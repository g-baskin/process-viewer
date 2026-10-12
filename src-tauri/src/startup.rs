//! Things that start automatically: launchd agents and daemons installed
//! by apps and installers (Apple's own in /System are left out).

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Serialize, Clone)]
pub struct StartupItem {
    pub label: String,
    pub plist: String,
    pub program: Option<String>,
    pub arguments: Vec<String>,
    /// "user" (~/Library/LaunchAgents), "all-users" (/Library/LaunchAgents),
    /// "system-service" (/Library/LaunchDaemons, runs as root).
    pub scope: &'static str,
    pub enabled: bool,
    /// When it runs, in plain words.
    pub when: String,
    pub vendor: Option<String>,
    pub running_pid: Option<u32>,
    pub program_exists: bool,
}

fn dirs() -> Vec<(PathBuf, &'static str)> {
    let mut v = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        v.push((Path::new(&home).join("Library/LaunchAgents"), "user"));
    }
    v.push((PathBuf::from("/Library/LaunchAgents"), "all-users"));
    v.push((PathBuf::from("/Library/LaunchDaemons"), "system-service"));
    v
}

extern "C" {
    fn getuid() -> u32;
}
fn uid() -> u32 {
    unsafe { getuid() }
}

/// Parse `launchctl print-disabled <domain>` into label -> disabled.
fn overrides(domain: &str) -> HashMap<String, bool> {
    let mut map = HashMap::new();
    let Ok(out) = Command::new("/bin/launchctl")
        .args(["print-disabled", domain])
        .output()
    else {
        return map;
    };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let line = line.trim();
        let Some((l, r)) = line.split_once("=>") else {
            continue;
        };
        let label = l.trim().trim_matches('"').to_string();
        let r = r.trim();
        let disabled = r == "disabled" || r == "true";
        map.insert(label, disabled);
    }
    map
}

fn when_text(d: &plist::Dictionary, scope: &str) -> String {
    let at = if scope == "system-service" {
        "at startup"
    } else {
        "at login"
    };
    let keep = match d.get("KeepAlive") {
        Some(plist::Value::Boolean(b)) => *b,
        Some(plist::Value::Dictionary(_)) => true,
        _ => false,
    };
    let run_at_load = matches!(d.get("RunAtLoad"), Some(plist::Value::Boolean(true)));
    let mut parts = Vec::new();
    if keep {
        parts.push(format!("Starts {at} and is kept running"));
    } else if run_at_load {
        parts.push(format!("Runs {at}"));
    }
    if let Some(n) = d.get("StartInterval").and_then(|v| v.as_signed_integer()) {
        parts.push(if n >= 3600 && n % 3600 == 0 {
            format!("every {} hour(s)", n / 3600)
        } else if n >= 60 {
            format!("every {} min", n / 60)
        } else {
            format!("every {n} s")
        });
    }
    if d.contains_key("StartCalendarInterval") {
        parts.push("on a schedule".into());
    }
    if d.contains_key("WatchPaths") || d.contains_key("QueueDirectories") {
        parts.push("when certain files change".into());
    }
    if parts.is_empty() {
        return "Only when something asks for it (on demand)".into();
    }
    let mut s = parts.join(", ");
    if let Some(f) = s.get_mut(0..1) {
        f.make_ascii_uppercase();
    }
    s
}

fn vendor(label: &str, program: Option<&str>) -> Option<String> {
    use crate::describe::{outer_bundle_name, vendor_from_path, vendor_from_reverse_dns};
    if let Some(p) = program {
        // An app in /Applications is the clearest name ("Docker", "AnyDesk").
        if let Some(rest) = p.strip_prefix("/Applications/") {
            if let Some(app) = outer_bundle_name(rest) {
                return Some(app.trim_end_matches(".localized").to_string());
            }
        }
        if let Some(v) = vendor_from_path(p) {
            return Some(v);
        }
    }
    vendor_from_reverse_dns(label).or_else(|| program.and_then(outer_bundle_name))
}

/// A running process as seen by the caller: (pid, program path, command line).
pub type RunningProc = (u32, String, String);

/// label -> pid for agents loaded in the user's own launchd domain.
fn loaded_agents() -> HashMap<String, u32> {
    let mut map = HashMap::new();
    let Ok(out) = Command::new("/bin/launchctl").arg("list").output() else {
        return map;
    };
    for line in String::from_utf8_lossy(&out.stdout).lines().skip(1) {
        let mut cols = line.split('\t');
        let (Some(pid), Some(_), Some(label)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        if let Ok(pid) = pid.parse::<u32>() {
            map.insert(label.to_string(), pid);
        }
    }
    map
}

const INTERPRETERS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "env",
    "python",
    "python3",
    "node",
    "ruby",
    "perl",
    "osascript",
];

/// For daemons (whose launchd domain we can't read without admin rights):
/// find the process by its program file. Scripts run by an interpreter
/// must also match the script path, so any random python isn't picked.
fn match_by_program(program: &str, args: &[String], procs: &[RunningProc]) -> Option<u32> {
    let base = program.rsplit('/').next().unwrap_or(program);
    let interp = INTERPRETERS.contains(&base) || base.starts_with("python");
    let script = args.iter().skip(1).find(|a| a.starts_with('/'));
    procs
        .iter()
        .find(|(_, exe, cmd)| {
            exe == program && (!interp || script.is_some_and(|s| cmd.contains(s.as_str())))
        })
        .map(|(pid, _, _)| *pid)
}

pub fn list(procs: &[RunningProc]) -> Vec<StartupItem> {
    let loaded = loaded_agents();
    let user_over = overrides(&format!("gui/{}", uid()));
    let sys_over = overrides("system");
    let mut items = Vec::new();
    for (dir, scope) in dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("plist") {
                continue;
            }
            let Ok(plist::Value::Dictionary(d)) = plist::Value::from_file(&path) else {
                continue;
            };
            let label = d
                .get("Label")
                .and_then(|v| v.as_string())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                });
            let arguments: Vec<String> = d
                .get("ProgramArguments")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_string().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let program = d
                .get("Program")
                .and_then(|v| v.as_string())
                .map(str::to_string)
                .or_else(|| arguments.first().cloned());
            let plist_disabled = matches!(d.get("Disabled"), Some(plist::Value::Boolean(true)));
            let over = if scope == "system-service" {
                &sys_over
            } else {
                &user_over
            };
            let enabled = !over.get(&label).copied().unwrap_or(plist_disabled);
            let running_pid = if scope == "system-service" {
                program
                    .as_deref()
                    .and_then(|p| match_by_program(p, &arguments, procs))
            } else {
                loaded.get(&label).copied()
            };
            let program_exists = program.as_deref().is_some_and(|p| Path::new(p).exists());
            items.push(StartupItem {
                vendor: vendor(&label, program.as_deref()),
                when: when_text(&d, scope),
                label,
                plist: path.to_string_lossy().into_owned(),
                program,
                arguments,
                scope,
                enabled,
                running_pid,
                program_exists,
            });
        }
    }
    items.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    items
}

fn safe_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() < 200
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// Turn a startup item on or off. Looks the item up again by label so only
/// items this app listed can be touched.
pub fn set_enabled(label: &str, enable: bool) -> Result<String, String> {
    if !safe_label(label) {
        return Err("This item has an unusual name, so it can't be changed from here. Use “Show in Finder” instead.".into());
    }
    let item = list(&[])
        .into_iter()
        .find(|i| i.label == label)
        .ok_or("That startup item no longer exists.")?;
    let plist_path = item.plist.clone();

    if item.scope == "system-service" && crate::privhelper::ready() {
        crate::privhelper::toggle(label, &plist_path, enable)?;
    } else if item.scope == "system-service" {
        // Needs root: ask macOS for the admin password. The label was checked
        // above; the plist path comes from our own listing of /Library/LaunchDaemons.
        let quoted = plist_path.replace('\'', "");
        let shell = if enable {
            format!("launchctl enable system/{label}; launchctl bootstrap system '{quoted}' 2>/dev/null; true")
        } else {
            format!("launchctl disable system/{label}; launchctl bootout system/{label} 2>/dev/null; true")
        };
        let verb = if enable { "turn on" } else { "turn off" };
        let script = format!(
            "do shell script \"{shell}\" with administrator privileges with prompt \"Process Viewer wants to {verb} the startup item “{label}”.\""
        );
        let out = Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(script)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(if err.contains("-128") {
                "Cancelled.".into()
            } else {
                err.trim().to_string()
            });
        }
    } else {
        let domain = format!("gui/{}", uid());
        let target = format!("{domain}/{label}");
        let first = Command::new("/bin/launchctl")
            .args([if enable { "enable" } else { "disable" }, &target])
            .output()
            .map_err(|e| e.to_string())?;
        if !first.status.success() {
            return Err(format!(
                "macOS refused: {}",
                String::from_utf8_lossy(&first.stderr).trim()
            ));
        }
        // Load or unload it now too; it's fine if it was already in that state.
        let _ = if enable {
            Command::new("/bin/launchctl")
                .args(["bootstrap", &domain, &plist_path])
                .output()
        } else {
            Command::new("/bin/launchctl")
                .args(["bootout", &target])
                .output()
        };
    }
    Ok(if enable {
        format!(
            "Turned on “{label}”. It will start {}.",
            if item.scope == "system-service" {
                "at startup"
            } else {
                "at login"
            }
        )
    } else {
        format!("Turned off “{label}”. It won't start automatically any more (you can turn it back on here).")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert!(safe_label("com.wacom.IOManager"));
        assert!(!safe_label("a; rm -rf /"));
        assert!(!safe_label("a\"b"));
    }

    #[test]
    fn interpreter_scripts_need_the_script_to_match() {
        let procs = vec![
            (
                10,
                "/usr/bin/python3".to_string(),
                "/usr/bin/python3 /other.py".to_string(),
            ),
            (
                11,
                "/usr/bin/python3".to_string(),
                "/usr/bin/python3 /mine.py".to_string(),
            ),
            (12, "/opt/x/daemon".to_string(), "/opt/x/daemon".to_string()),
        ];
        let args = vec!["/usr/bin/python3".to_string(), "/mine.py".to_string()];
        assert_eq!(
            match_by_program("/usr/bin/python3", &args, &procs),
            Some(11)
        );
        assert_eq!(
            match_by_program("/usr/bin/python3", &args[..1], &procs),
            None
        );
        assert_eq!(match_by_program("/opt/x/daemon", &[], &procs), Some(12));
    }

    #[test]
    fn when() {
        let mut d = plist::Dictionary::new();
        assert!(when_text(&d, "user").contains("on demand"));
        d.insert("RunAtLoad".into(), true.into());
        assert_eq!(when_text(&d, "user"), "Runs at login");
        d.insert("KeepAlive".into(), true.into());
        d.insert("StartInterval".into(), plist::Value::Integer(3600.into()));
        assert_eq!(
            when_text(&d, "system-service"),
            "Starts at startup and is kept running, every 1 hour(s)"
        );
    }

    #[test]
    fn lists_real_items_without_crashing() {
        let items = list(&[]);
        assert!(items.iter().all(|i| !i.label.is_empty()));
    }
}
