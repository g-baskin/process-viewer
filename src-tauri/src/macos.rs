//! macOS-only pieces: the list of open apps (as the Dock sees them),
//! graceful app quitting, app icons and the "responsible process".
//!
//! AppKit calls here must run on the main thread; callers in lib.rs use
//! `run_on_main_thread` for that.

use objc2_app_kit::{
    NSApplicationActivationPolicy, NSBitmapImageFileType, NSBitmapImageRep, NSRunningApplication,
    NSWorkspace,
};
use objc2_foundation::{NSDictionary, NSString};
use serde::Serialize;
use std::ffi::{c_char, c_int, c_void, CString};

#[derive(Clone, Serialize)]
pub struct RunningApp {
    pub pid: u32,
    pub name: String,
    pub bundle_id: Option<String>,
    pub bundle_path: Option<String>,
    /// "regular" = has a Dock icon, "accessory" = menu bar / background UI app.
    pub policy: &'static str,
}

/// Apps that macOS considers "open", i.e. what the Dock and Cmd-Tab see.
pub fn running_apps() -> Vec<RunningApp> {
    let ws = NSWorkspace::sharedWorkspace();
    let apps = ws.runningApplications();
    let mut out = Vec::new();
    for app in apps.iter() {
        let policy = app.activationPolicy();
        let policy = if policy == NSApplicationActivationPolicy::Regular {
            "regular"
        } else if policy == NSApplicationActivationPolicy::Accessory {
            "accessory"
        } else {
            continue;
        };
        let pid = app.processIdentifier();
        if pid <= 0 {
            continue;
        }
        let bundle_path = app
            .bundleURL()
            .and_then(|u| u.path())
            .map(|s| s.to_string());
        let name = app
            .localizedName()
            .map(|s| s.to_string())
            .or_else(|| {
                bundle_path
                    .as_deref()
                    .and_then(crate::describe::outer_bundle_name)
            })
            .unwrap_or_else(|| format!("App {pid}"));
        out.push(RunningApp {
            pid: pid as u32,
            name,
            bundle_id: app.bundleIdentifier().map(|s| s.to_string()),
            bundle_path,
            policy,
        });
    }
    out
}

/// Ask an app to quit the normal way (like Cmd-Q), so it can ask to save.
/// Returns None if the pid is not an app.
pub fn quit_app(pid: u32, force: bool) -> Option<bool> {
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32)?;
    Some(if force {
        app.forceTerminate()
    } else {
        app.terminate()
    })
}

/// PNG bytes of the Finder icon for a file or bundle path, about 64px.
pub fn icon_png(path: &str) -> Option<Vec<u8>> {
    let ws = NSWorkspace::sharedWorkspace();
    let image = ws.iconForFile(&NSString::from_str(path));
    // Pick the representation closest to 64x64 points (128px on Retina).
    let reps = image.representations();
    let mut best: Option<(f64, objc2::rc::Retained<objc2_app_kit::NSImageRep>)> = None;
    for rep in reps.iter() {
        let w = rep.pixelsWide() as f64;
        let score = (w - 128.0).abs();
        if best.as_ref().is_none_or(|(s, _)| score < *s) {
            best = Some((score, rep));
        }
    }
    let (_, rep) = best?;
    let bitmap = rep.downcast::<NSBitmapImageRep>().ok();
    let bitmap = match bitmap {
        Some(b) => b,
        None => {
            let tiff = image.TIFFRepresentation()?;
            NSBitmapImageRep::imageRepWithData(&tiff)?
        }
    };
    let data = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }?;
    Some(data.to_vec())
}

type RespFn = unsafe extern "C" fn(c_int) -> c_int;

extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

/// The process macOS holds "responsible" for `pid` (the same idea Activity
/// Monitor uses: an XPC helper is attributed to the app that asked for it).
/// Looked up at runtime because the function is not in a public header.
pub fn responsible_pid(pid: u32) -> Option<u32> {
    use std::sync::OnceLock;
    static FUNC: OnceLock<Option<RespFn>> = OnceLock::new();
    let f = FUNC.get_or_init(|| {
        let name = CString::new("responsibility_get_pid_responsible_for_pid").ok()?;
        let p = unsafe { dlsym(RTLD_DEFAULT, name.as_ptr()) };
        if p.is_null() {
            None
        } else {
            Some(unsafe { std::mem::transmute::<*mut c_void, RespFn>(p) })
        }
    });
    let f = (*f)?;
    let r = unsafe { f(pid as c_int) };
    if r > 0 {
        Some(r as u32)
    } else {
        None
    }
}
