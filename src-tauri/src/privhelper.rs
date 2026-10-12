//! Password-free switching of system startup items (launch daemons).
//!
//! One admin prompt installs:
//! - a small root-owned script in /Library/PrivilegedHelperTools that can only
//!   enable/disable a third-party daemon whose plist sits directly in
//!   /Library/LaunchDaemons and whose Label matches, and
//! - a sudoers rule letting the current user run just that script without a
//!   password.
//!
//! After that the app calls `sudo -n <helper> …` and never prompts again.

use std::path::Path;
use std::process::Command;

pub const HELPER: &str =
    "/Library/PrivilegedHelperTools/com.dellcbyer.processviewer.startup-helper";
pub const SUDOERS: &str = "/etc/sudoers.d/process-viewer";

pub const SCRIPT: &str = r#"#!/bin/sh
# Process Viewer startup helper (v1).
# Turns third-party launch daemons in /Library/LaunchDaemons on or off.
# Installed by Process Viewer. To remove it, use "Remove" in the app's Startup
# tab, or run: sudo rm /etc/sudoers.d/process-viewer "$0"
set -u
PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
[ "$#" -eq 3 ] || { echo "usage: $0 enable|disable LABEL PLIST" >&2; exit 2; }
action=$1; label=$2; plist=$3
case "$action" in enable|disable) ;; *) echo "bad action" >&2; exit 2 ;; esac
case "$label" in ''|*[!A-Za-z0-9._-]*|com.apple.*) echo "bad label" >&2; exit 2 ;; esac
case "$plist" in /Library/LaunchDaemons/*.plist) ;; *) echo "bad plist path" >&2; exit 2 ;; esac
name=${plist#/Library/LaunchDaemons/}
case "$name" in */*|.*) echo "bad plist path" >&2; exit 2 ;; esac
if [ ! -f "$plist" ] || [ -L "$plist" ]; then echo "no such plist" >&2; exit 2; fi
actual=$(/usr/libexec/PlistBuddy -c "Print :Label" "$plist" 2>/dev/null) || { echo "unreadable plist" >&2; exit 2; }
[ "$actual" = "$label" ] || { echo "label does not match plist" >&2; exit 2; }
if [ "$action" = disable ]; then
  launchctl disable "system/$label" || exit 1
  launchctl bootout "system/$label" 2>/dev/null
else
  launchctl enable "system/$label" || exit 1
  launchctl bootstrap system "$plist" 2>/dev/null
fi
exit 0
"#;

fn username() -> Result<String, String> {
    let out = Command::new("/usr/bin/id")
        .arg("-un")
        .output()
        .map_err(|e| e.to_string())?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err("Couldn't work out your user name.".into());
    }
    Ok(name)
}

fn sudoers_line(user: &str) -> String {
    format!(
        "# Added by Process Viewer: lets {user} turn third-party startup services on/off without a password.\n\
         {user} ALL=(root) NOPASSWD: {HELPER}\n"
    )
}

/// True when the helper is installed, up to date, and sudo will run it
/// without asking for a password.
pub fn ready() -> bool {
    let current = std::fs::read_to_string(HELPER).ok();
    if current.as_deref() != Some(SCRIPT) {
        return false;
    }
    Command::new("/usr/bin/sudo")
        .args(["-n", "-l", HELPER])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run a shell command as root after one macOS password prompt.
/// `shell` must only contain fixed paths, checked labels and numbers: it is
/// rejected if it holds anything that could break out of the AppleScript
/// string. The prompt is display text, so such characters are just removed.
pub(crate) fn run_as_admin(shell: &str, prompt: &str) -> Result<(), String> {
    if shell.contains('"') || shell.contains('\\') {
        return Err("Unexpected characters in the command.".into());
    }
    let prompt = prompt.replace(['"', '\\'], "");
    let script = format!(
        "do shell script \"{shell}\" with administrator privileges with prompt \"{prompt}\""
    );
    let out = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(if err.contains("-128") {
            "Cancelled.".into()
        } else {
            err.trim().to_string()
        })
    }
}

/// Install the helper and the sudoers rule (one password prompt).
pub fn install() -> Result<(), String> {
    let user = username()?;
    let dir = std::env::temp_dir().join(format!("process-viewer-setup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let helper_tmp = dir.join("helper");
    let sudoers_tmp = dir.join("sudoers");
    std::fs::write(&helper_tmp, SCRIPT).map_err(|e| e.to_string())?;
    std::fs::write(&sudoers_tmp, sudoers_line(&user)).map_err(|e| e.to_string())?;
    let (h, s) = (helper_tmp.to_string_lossy(), sudoers_tmp.to_string_lossy());
    if [h.as_ref(), s.as_ref()].iter().any(|p| p.contains('\'')) {
        return Err("The temporary folder has an unusual name.".into());
    }
    // visudo checks the rule first, so a broken file can never reach sudoers.d.
    let shell = format!(
        "set -e; /usr/sbin/visudo -cf '{s}' >/dev/null; \
         /usr/bin/install -d -o root -g wheel -m 755 /Library/PrivilegedHelperTools; \
         /usr/bin/install -o root -g wheel -m 755 '{h}' '{HELPER}'; \
         /usr/bin/install -o root -g wheel -m 440 '{s}' '{SUDOERS}'"
    );
    let r = run_as_admin(
        &shell,
        "Process Viewer wants to turn system services on and off without asking for your password each time. You only need to do this once.",
    );
    let _ = std::fs::remove_dir_all(&dir);
    r?;
    if !ready() {
        return Err("Setup finished but sudo still asks for a password. Check that /etc/sudoers includes /etc/sudoers.d.".into());
    }
    Ok(())
}

/// Remove the helper and the sudoers rule (one password prompt).
pub fn remove() -> Result<(), String> {
    run_as_admin(
        &format!("/bin/rm -f '{SUDOERS}' '{HELPER}'"),
        "Process Viewer wants to remove its permission to manage system services.",
    )
}

/// Turn a system service on or off through the startup helper, without a prompt.
pub fn toggle(label: &str, plist: &str, enable: bool) -> Result<(), String> {
    if !Path::new(plist).starts_with("/Library/LaunchDaemons") {
        return Err("Only items in /Library/LaunchDaemons can be changed this way.".into());
    }
    let out = Command::new("/usr/bin/sudo")
        .args([
            "-n",
            HELPER,
            if enable { "enable" } else { "disable" },
            label,
            plist,
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "macOS refused: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(args: &[&str]) -> (i32, String) {
        // Run the helper script as the normal user: validation happens before
        // any launchctl call, so rejected input must exit 2 and change nothing.
        let out = Command::new("/bin/sh")
            .arg("-c")
            .arg(SCRIPT)
            .arg("helper")
            .args(args)
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    #[test]
    fn helper_rejects_bad_input() {
        assert_eq!(sh(&[]).0, 2);
        assert_eq!(sh(&["rm", "x.y", "/Library/LaunchDaemons/x.plist"]).0, 2);
        assert_eq!(
            sh(&["disable", "a;b", "/Library/LaunchDaemons/x.plist"]).0,
            2
        );
        assert_eq!(
            sh(&["disable", "com.apple.foo", "/Library/LaunchDaemons/x.plist"]).0,
            2
        );
        assert_eq!(
            sh(&["disable", "x.y", "/System/Library/LaunchDaemons/x.plist"]).0,
            2
        );
        assert_eq!(
            sh(&["disable", "x.y", "/Library/LaunchDaemons/../x.plist"]).0,
            2
        );
        assert_eq!(
            sh(&["disable", "x.y", "/Library/LaunchDaemons/missing-xyz.plist"]).0,
            2
        );
    }

    #[test]
    fn helper_rejects_label_mismatch() {
        // A real daemon plist, but with the wrong label.
        let Some(entry) = std::fs::read_dir("/Library/LaunchDaemons")
            .ok()
            .and_then(|mut d| d.find_map(|e| e.ok()))
        else {
            return;
        };
        let p = entry.path();
        let (code, err) = sh(&["disable", "not.the.right.label", p.to_str().unwrap()]);
        assert_eq!(code, 2, "{err}");
    }

    #[test]
    fn sudoers_rule_is_valid() {
        let line = sudoers_line("someone");
        let f = std::env::temp_dir().join("pv-sudoers-test");
        std::fs::write(&f, &line).unwrap();
        let out = Command::new("/usr/sbin/visudo")
            .arg("-cf")
            .arg(&f)
            .output()
            .unwrap();
        let _ = std::fs::remove_file(&f);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
