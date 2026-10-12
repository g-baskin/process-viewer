mod describe;
mod privhelper;
mod startup;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
mod macos {
    //! Stubs so the crate still builds elsewhere; the app targets macOS.
    #[derive(Clone, serde::Serialize)]
    pub struct RunningApp {
        pub pid: u32,
        pub name: String,
        pub bundle_id: Option<String>,
        pub bundle_path: Option<String>,
        pub policy: &'static str,
    }
    pub fn running_apps() -> Vec<RunningApp> {
        Vec::new()
    }
    pub fn quit_app(_: u32, _: bool) -> Option<bool> {
        None
    }
    pub fn icon_png(_: &str) -> Option<Vec<u8>> {
        None
    }
    pub fn responsible_pid(_: u32) -> Option<u32> {
        None
    }
}

use base64::Engine;
use macos::RunningApp;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, Users};
use tauri::{AppHandle, State};

struct AppState {
    sys: Mutex<System>,
    users: Mutex<Users>,
    icons: Mutex<HashMap<String, Option<String>>>,
}

#[derive(Serialize, Clone)]
struct Ancestor {
    pid: u32,
    name: String,
}

#[derive(Serialize)]
struct Proc {
    pid: u32,
    ppid: Option<u32>,
    name: String,
    exe: Option<String>,
    command: String,
    user: Option<String>,
    is_root: bool,
    cpu: f32,
    memory: u64,
    start_time: u64,
    run_time: u64,
    /// "app" | "menubar" | "helper" | "background" | "system"
    kind: &'static str,
    /// The open app this process belongs to (itself for apps).
    app_pid: Option<u32>,
    app_name: Option<String>,
    description: String,
    protected: bool,
    /// Parent, grandparent, … up to launchd.
    launched_by: Vec<Ancestor>,
    /// The process macOS holds responsible, when it differs from the parent.
    responsible: Option<Ancestor>,
}

#[derive(Serialize)]
struct Snapshot {
    procs: Vec<Proc>,
    apps: Vec<RunningApp>,
    total_memory: u64,
    cpu_count: usize,
    self_pid: u32,
}

extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn getuid() -> u32;
}
const SIGTERM: i32 = 15;
const SIGKILL: i32 = 9;

/// Run `f` on the main thread (AppKit needs it) and wait for the result.
/// Only call from commands marked `async`, which run off the main thread.
fn on_main<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())
}

fn refresh_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_cpu()
        .with_memory()
        .with_exe(UpdateKind::OnlyIfNotSet)
        .with_cmd(UpdateKind::OnlyIfNotSet)
        .with_user(UpdateKind::OnlyIfNotSet)
}

fn is_system_path(exe: &str) -> bool {
    ["/System/", "/usr/", "/sbin/", "/bin/", "/Library/Apple/"]
        .iter()
        .any(|p| exe.starts_with(p))
        && !exe.starts_with("/usr/local/")
}

#[tauri::command(async)]
fn snapshot(app: AppHandle, state: State<'_, AppState>) -> Result<Snapshot, String> {
    let apps = on_main(&app, macos::running_apps)?;
    build_snapshot(&state, apps)
}

fn is_apple_bundle(a: &RunningApp) -> bool {
    a.bundle_id
        .as_deref()
        .is_some_and(|b| b.starts_with("com.apple."))
        || a.bundle_path.as_deref().is_none_or(|p| {
            p.starts_with("/System/") || p.starts_with("/Library/Apple/") || p.ends_with(".xpc")
        })
}

/// Keep the apps a person thinks of as "apps": every Dock app, plus
/// third-party menu bar apps that aren't just a piece of another app.
fn top_level_apps(mut apps: Vec<RunningApp>) -> Vec<RunningApp> {
    // Regular apps first so they win when two share a bundle, then
    // outer bundles before the ones nested inside them.
    apps.sort_by_key(|a| {
        (
            a.policy != "regular",
            a.bundle_path.as_ref().map_or(usize::MAX, |p| p.len()),
            a.pid,
        )
    });
    let mut kept: Vec<RunningApp> = Vec::new();
    for a in apps {
        if a.policy == "regular" {
            kept.push(a);
            continue;
        }
        if is_apple_bundle(&a) {
            continue;
        }
        let path = a.bundle_path.as_deref().unwrap_or("");
        let nested = kept.iter().any(|k| {
            k.bundle_path
                .as_deref()
                .is_some_and(|kp| path == kp || path.starts_with(&format!("{kp}/")))
        });
        if !nested {
            kept.push(a);
        }
    }
    kept
}

fn build_snapshot(state: &AppState, apps: Vec<RunningApp>) -> Result<Snapshot, String> {
    let apps = top_level_apps(apps);
    let mut sys = state.sys.lock().map_err(|e| e.to_string())?;
    sys.refresh_memory();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind());
    let users = state.users.lock().map_err(|e| e.to_string())?;
    let my_uid = unsafe { getuid() };

    let app_by_pid: HashMap<u32, &RunningApp> = apps.iter().map(|a| (a.pid, a)).collect();
    let bundle_to_app: HashMap<&str, u32> = apps
        .iter()
        .filter_map(|a| a.bundle_path.as_deref().map(|b| (b, a.pid)))
        .collect();
    let names: HashMap<u32, String> = sys
        .processes()
        .iter()
        .map(|(pid, p)| (pid.as_u32(), p.name().to_string_lossy().into_owned()))
        .collect();
    let parents: HashMap<u32, u32> = sys
        .processes()
        .iter()
        .filter_map(|(pid, p)| p.parent().map(|pp| (pid.as_u32(), pp.as_u32())))
        .collect();

    let mut procs = Vec::with_capacity(sys.processes().len());
    for (pid, p) in sys.processes() {
        let pid = pid.as_u32();
        let name = names.get(&pid).cloned().unwrap_or_default();
        let exe = p
            .exe()
            .map(|e| e.to_string_lossy().into_owned())
            .filter(|e| !e.is_empty());
        let ppid = p.parent().map(|x| x.as_u32());

        // Parent chain.
        let mut launched_by = Vec::new();
        let mut cur = ppid;
        while let Some(c) = cur {
            if launched_by.len() >= 32 || c == pid {
                break;
            }
            launched_by.push(Ancestor {
                pid: c,
                name: names.get(&c).cloned().unwrap_or_else(|| "?".into()),
            });
            if c <= 1 {
                break;
            }
            cur = parents.get(&c).copied();
        }

        let resp = macos::responsible_pid(pid).filter(|r| *r != pid);
        let responsible = resp.filter(|r| Some(*r) != ppid).map(|r| Ancestor {
            pid: r,
            name: names.get(&r).cloned().unwrap_or_else(|| "?".into()),
        });

        // Which open app does this belong to?
        let app_pid = if app_by_pid.contains_key(&pid) {
            Some(pid)
        } else if let Some(r) = resp.filter(|r| app_by_pid.contains_key(r)) {
            Some(r)
        } else if let Some(a) = launched_by.iter().find(|a| app_by_pid.contains_key(&a.pid)) {
            Some(a.pid)
        } else {
            exe.as_deref()
                .and_then(describe::outer_bundle_path)
                .and_then(|b| bundle_to_app.get(b.as_str()).copied())
        };
        let app_name = app_pid
            .and_then(|a| app_by_pid.get(&a))
            .map(|a| a.name.clone());

        let uid = p.user_id().map(|u| **u);
        let user = p
            .user_id()
            .and_then(|u| users.get_user_by_id(u))
            .map(|u| u.name().to_string());
        let is_root = uid == Some(0);

        let kind: &'static str = match app_by_pid.get(&pid) {
            Some(a) if a.policy == "regular" => "app",
            Some(_) => "menubar",
            None if app_pid.is_some() => "helper",
            None => {
                let system = pid <= 1
                    || is_root
                    || user.as_deref().is_some_and(|u| u.starts_with('_'))
                    || exe.as_deref().is_some_and(is_system_path)
                    || (exe.is_none() && uid != Some(my_uid));
                if system {
                    "system"
                } else {
                    "background"
                }
            }
        };

        let description = describe::describe(&describe::Ctx {
            name: &name,
            exe: exe.as_deref(),
            kind,
            app_name: app_name.as_deref(),
            is_root,
        });

        let mut command = String::new();
        for (i, arg) in p.cmd().iter().enumerate() {
            if i > 0 {
                command.push(' ');
            }
            command.push_str(&arg.to_string_lossy());
            if command.len() > 2000 {
                command.push_str(" …");
                break;
            }
        }

        procs.push(Proc {
            pid,
            ppid,
            protected: describe::is_protected(pid, &name),
            name,
            exe,
            command,
            user,
            is_root,
            cpu: p.cpu_usage(),
            memory: p.memory(),
            start_time: p.start_time(),
            run_time: p.run_time(),
            kind,
            app_pid,
            app_name,
            description,
            launched_by,
            responsible,
        });
    }

    Ok(Snapshot {
        procs,
        apps,
        total_memory: sys.total_memory(),
        cpu_count: sys.cpus().len().max(
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
        ),
        self_pid: std::process::id(),
    })
}

/// Check that `pid` is still the process the user picked and may be stopped.
/// Returns its name.
fn check_target(state: &AppState, pid: u32, start_time: u64) -> Result<String, String> {
    let name = {
        let mut sys = state.sys.lock().map_err(|e| e.to_string())?;
        let target = Pid::from_u32(pid);
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[target]), true, refresh_kind());
        let p = sys
            .process(target)
            .ok_or("That process has already ended.")?;
        // Guard against the pid being reused by a new process since the list was shown.
        if p.start_time() != start_time {
            return Err("That process has already ended (its ID now belongs to a different process). Refresh and try again.".into());
        }
        p.name().to_string_lossy().into_owned()
    };
    if describe::is_protected(pid, &name) {
        return Err(format!("“{name}” is a core part of macOS. Stopping it would log you out or freeze the Mac, so this app won't do it."));
    }
    if pid == std::process::id() {
        return Err("That's this app. Close the window to quit it.".into());
    }
    Ok(name)
}

/// Stop one checked process without a password ("quit" or "force").
fn stop_one(app: &AppHandle, pid: u32, name: &str, mode: &str) -> Result<String, String> {
    match mode {
        "quit" => {
            if let Some(ok) = on_main(app, move || macos::quit_app(pid, false))? {
                return if ok {
                    Ok(format!(
                        "Asked “{name}” to quit. It may ask you to save your work first."
                    ))
                } else {
                    Err(format!(
                        "“{name}” didn't accept the quit request. Try Force Quit."
                    ))
                };
            }
            send_signal(pid, SIGTERM, name).map(|_| format!("Sent “{name}” a request to stop."))
        }
        "force" => send_signal(pid, SIGKILL, name).map(|_| format!("Force quit “{name}”.")),
        _ => Err("Unknown action.".into()),
    }
}

/// Force quit checked processes as root with a single password prompt.
/// pids are numbers, so they are safe to put in the shell command.
fn kill_as_admin(targets: &[(u32, String)]) -> Result<(), String> {
    let pids: Vec<String> = targets.iter().map(|(p, _)| p.to_string()).collect();
    let prompt = if let [(_, name)] = targets {
        format!("Process Viewer wants to force quit “{name}”.")
    } else {
        format!(
            "Process Viewer wants to force quit {} processes.",
            targets.len()
        )
    };
    privhelper::run_as_admin(&format!("kill -9 {}", pids.join(" ")), &prompt).map_err(|e| {
        if e == "Cancelled." {
            e
        } else {
            format!("Couldn't stop them: {e}")
        }
    })
}

/// mode: "quit" (polite), "force" (SIGKILL), "admin" (SIGKILL via an
/// administrator password prompt, for processes owned by root).
#[tauri::command(async)]
fn terminate(
    app: AppHandle,
    state: State<'_, AppState>,
    pid: u32,
    start_time: u64,
    mode: String,
) -> Result<String, String> {
    let out = terminate_many(app, state, vec![Target { pid, start_time }], mode)?;
    let o = out.into_iter().next().ok_or("Nothing was stopped.")?;
    if o.ok {
        Ok(o.message)
    } else {
        Err(o.message)
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
    pid: u32,
    start_time: u64,
}

#[derive(Serialize)]
struct StopOutcome {
    pid: u32,
    ok: bool,
    message: String,
}

/// Stop several processes. Each one is checked again (still the same
/// process, not protected, not this app). "admin" asks for the password once
/// for all of them. Returns one outcome per target, in the order given.
#[tauri::command(async)]
fn terminate_many(
    app: AppHandle,
    state: State<'_, AppState>,
    targets: Vec<Target>,
    mode: String,
) -> Result<Vec<StopOutcome>, String> {
    if !matches!(mode.as_str(), "quit" | "force" | "admin") {
        return Err("Unknown action.".into());
    }
    if targets.len() > 500 {
        return Err("Too many processes at once.".into());
    }
    let mut out = Vec::with_capacity(targets.len());
    let mut admin: Vec<(usize, u32, String)> = Vec::new();
    for t in &targets {
        let checked = check_target(&state, t.pid, t.start_time);
        match (checked, mode.as_str()) {
            (Err(message), _) => out.push(StopOutcome {
                pid: t.pid,
                ok: false,
                message,
            }),
            (Ok(name), "admin") => {
                admin.push((out.len(), t.pid, name));
                out.push(StopOutcome {
                    pid: t.pid,
                    ok: false,
                    message: String::new(),
                });
            }
            (Ok(name), m) => {
                let r = stop_one(&app, t.pid, &name, m);
                out.push(StopOutcome {
                    pid: t.pid,
                    ok: r.is_ok(),
                    message: r.unwrap_or_else(|e| e),
                });
            }
        }
    }
    if !admin.is_empty() {
        let list: Vec<(u32, String)> = admin.iter().map(|(_, p, n)| (*p, n.clone())).collect();
        let r = kill_as_admin(&list);
        for (i, _, name) in admin {
            out[i].ok = r.is_ok();
            out[i].message = match &r {
                Ok(()) => format!("Force quit “{name}” as administrator."),
                Err(e) => e.clone(),
            };
        }
    }
    Ok(out)
}

fn send_signal(pid: u32, sig: i32, name: &str) -> Result<(), String> {
    let r = unsafe { kill(pid as i32, sig) };
    if r == 0 {
        return Ok(());
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(1) => Err(format!(
            "“{name}” belongs to the system or another user, so macOS needs your administrator password to stop it. Use “Force quit as admin”."
        )),
        Some(3) => Err(format!("“{name}” has already ended.")),
        _ => Err(format!("Couldn't stop “{name}”: {}", std::io::Error::last_os_error())),
    }
}

/// App/file icon as a data URL, cached by path.
#[tauri::command(async)]
fn icon(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<Option<String>, String> {
    if let Some(hit) = state.icons.lock().map_err(|e| e.to_string())?.get(&path) {
        return Ok(hit.clone());
    }
    if !std::path::Path::new(&path).exists() {
        return Ok(None);
    }
    let p = path.clone();
    let png = on_main(&app, move || macos::icon_png(&p))?;
    let url = png.map(|b| {
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(b)
        )
    });
    state
        .icons
        .lock()
        .map_err(|e| e.to_string())?
        .insert(path, url.clone());
    Ok(url)
}

#[tauri::command(async)]
fn startup_items(state: State<'_, AppState>) -> Result<Vec<startup::StartupItem>, String> {
    let procs: Vec<startup::RunningProc> = {
        let sys = state.sys.lock().map_err(|e| e.to_string())?;
        sys.processes()
            .iter()
            .filter_map(|(pid, p)| {
                let exe = p.exe()?.to_string_lossy().into_owned();
                let cmd = p
                    .cmd()
                    .iter()
                    .map(|a| a.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ");
                Some((pid.as_u32(), exe, cmd))
            })
            .collect()
    };
    Ok(startup::list(&procs))
}

#[tauri::command(async)]
fn set_startup(label: String, enable: bool) -> Result<String, String> {
    startup::set_enabled(&label, enable)
}

/// Turn several startup items on or off; system services share one prompt.
#[tauri::command(async)]
fn set_startup_many(labels: Vec<String>, enable: bool) -> Result<Vec<startup::Outcome>, String> {
    if labels.len() > 500 {
        return Err("Too many items at once.".into());
    }
    Ok(startup::set_many(&labels, enable))
}

/// Is password-free switching of system startup items set up?
#[tauri::command(async)]
fn startup_helper_ready() -> bool {
    privhelper::ready()
}

/// One password prompt, then system startup items switch without asking.
#[tauri::command(async)]
fn install_startup_helper() -> Result<(), String> {
    privhelper::install()
}

#[tauri::command(async)]
fn remove_startup_helper() -> Result<(), String> {
    privhelper::remove()
}

/// Show a program in Finder.
#[tauri::command]
fn reveal(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).exists() {
        return Err("That file no longer exists.".into());
    }
    std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Open a web search about a process in the default browser.
#[tauri::command]
fn search_web(name: String) -> Result<(), String> {
    let q = format!("what is \"{name}\" process on macOS");
    let mut enc = String::new();
    for b in q.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                enc.push(b as char)
            }
            b' ' => enc.push('+'),
            _ => enc.push_str(&format!("%{b:02X}")),
        }
    }
    std::process::Command::new("/usr/bin/open")
        .arg(format!("https://www.google.com/search?q={enc}"))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn new_state() -> AppState {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh_kind());
    AppState {
        sys: Mutex::new(sys),
        users: Mutex::new(Users::new_with_refreshed_list()),
        icons: Mutex::new(HashMap::new()),
    }
}

/// Debug aid: `process-app --dump` prints one snapshot as JSON and exits.
pub fn dump() {
    let state = new_state();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    let snap = build_snapshot(&state, macos::running_apps()).expect("snapshot");
    println!("{}", serde_json::to_string(&snap).expect("json"));
}

/// Debug aid: `process-app --dump-startup` prints the startup items as JSON.
pub fn dump_startup() {
    println!(
        "{}",
        serde_json::to_string(&startup::list(&[])).expect("json")
    );
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(new_state())
        .invoke_handler(tauri::generate_handler![
            snapshot,
            terminate,
            icon,
            reveal,
            search_web,
            startup_items,
            set_startup,
            set_startup_many,
            terminate_many,
            startup_helper_ready,
            install_startup_helper,
            remove_startup_helper
        ])
        .run(tauri::generate_context!())
        .expect("error while running Process Viewer");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_stop_a_child_and_report_gone() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("60")
            .spawn()
            .unwrap();
        let pid = child.id();
        assert!(send_signal(pid, SIGTERM, "sleep").is_ok());
        let status = child.wait().unwrap();
        assert!(!status.success());
        // Already reaped: a second signal reports it's gone.
        let err = send_signal(pid, SIGKILL, "sleep").unwrap_err();
        assert!(err.contains("already ended"), "{err}");
    }

    #[test]
    fn signalling_launchd_is_refused_by_the_os() {
        let err = send_signal(1, 0, "launchd").unwrap_err();
        assert!(err.contains("administrator"), "{err}");
    }
}
