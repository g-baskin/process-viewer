//! Plain-English descriptions for processes.
//!
//! First a table of well-known macOS processes, then heuristics based on
//! the process name, where its program lives on disk, and which app it
//! belongs to.

/// Processes that must never be terminated from this app: killing them
/// logs you out, freezes the screen or panics the machine.
pub const PROTECTED: &[&str] = &[
    "kernel_task",
    "launchd",
    "WindowServer",
    "loginwindow",
    "logd",
    "opendirectoryd",
    "configd",
    "notifyd",
    "securityd",
    "UserEventAgent",
    "kextd",
    "kernelmanagerd",
    "watchdogd",
    "diskarbitrationd",
    "fseventsd",
    "syspolicyd",
    "trustd",
    "powerd",
    "coreservicesd",
    "mDNSResponder",
    "cfprefsd",
    "distnoted",
    "runningboardd",
    "endpointsecurityd",
];

pub fn is_protected(pid: u32, name: &str) -> bool {
    pid <= 1 || PROTECTED.contains(&name)
}

/// Well-known processes, matched on the exact process name.
fn known(name: &str) -> Option<&'static str> {
    Some(match name {
        "kernel_task" => "The core of macOS (the kernel). It also uses CPU on purpose to cool the Mac down when it gets hot. Cannot be stopped.",
        "launchd" => "The first process macOS starts. It launches and supervises almost every other process, including apps and background services.",
        "WindowServer" => "Draws everything you see on screen and handles windows. Stopping it logs you out immediately.",
        "loginwindow" => "Manages your login session and the login screen. Stopping it logs you out.",
        "Finder" => "The Finder: the desktop, file browser and file windows.",
        "Dock" => "The Dock, Launchpad, Mission Control and the app switcher.",
        "SystemUIServer" => "Runs parts of the menu bar (older menu extras such as some status icons).",
        "ControlCenter" => "The Control Centre and the menu bar status items (Wi-Fi, Bluetooth, sound, battery).",
        "NotificationCenter" => "Shows notifications and the Notification Centre panel.",
        "Spotlight" => "The Spotlight search window.",
        "mds" => "Spotlight's metadata server: keeps the search index of your files up to date.",
        "mds_stores" => "Spotlight's index store. Busy after big file changes or updates; settles down on its own.",
        "mdworker" | "mdworker_shared" => "Spotlight worker: reads files to add them to the search index. Short-lived and harmless.",
        "mdsync" => "Spotlight index synchronisation helper.",
        "corespotlightd" => "Indexes content that apps hand to Spotlight (mail, messages, notes).",
        "coreaudiod" => "The system audio engine. Stopping it cuts sound briefly; macOS restarts it.",
        "bluetoothd" => "Manages Bluetooth devices (keyboards, mice, headphones).",
        "airportd" => "Manages Wi-Fi connections.",
        "wifid" | "wifip2pd" | "wifianalyticsd" | "wifivelocityd" => "Part of the Wi-Fi system.",
        "sharingd" => "AirDrop, Handoff, Universal Clipboard and other sharing features.",
        "rapportd" => "Lets your Apple devices find and talk to each other (Handoff, Continuity).",
        "bird" | "cloudd" => "iCloud Drive / CloudKit sync: keeps your iCloud files in step with the cloud.",
        "fileproviderd" => "Connects cloud storage (iCloud Drive, Dropbox, OneDrive, Google Drive) to Finder.",
        "photoanalysisd" | "photolibraryd" | "mediaanalysisd" => "Photos background analysis: faces, objects and memories. Runs while the Mac is idle.",
        "nsurlsessiond" => "Handles background downloads and uploads for apps and the system.",
        "softwareupdated" => "Checks for and installs macOS updates.",
        "backupd" | "backupd-helper" => "Time Machine backups.",
        "cfprefsd" => "Reads and writes app and system preferences. Needed by nearly everything.",
        "distnoted" => "Delivers notifications between processes. Needed by nearly everything.",
        "notifyd" => "System-wide notification service between processes.",
        "configd" => "Keeps track of network and system configuration.",
        "opendirectoryd" => "User accounts, groups and directory services (logins depend on it).",
        "securityd" => "Keychain and security services.",
        "trustd" => "Checks certificates for websites and signed software.",
        "syspolicyd" => "Gatekeeper: checks that apps you open are safe and signed.",
        "XProtect" | "XprotectService" | "XProtectPluginService" => "Apple's built-in malware scanner.",
        "logd" => "The system logging service.",
        "powerd" => "Power management: sleep, battery and energy settings.",
        "fseventsd" => "Watches file system changes so apps (and Spotlight, Time Machine) know what changed.",
        "diskarbitrationd" => "Mounts and unmounts disks and USB drives.",
        "mDNSResponder" => "DNS and Bonjour: turns web addresses into IP addresses. Stopping it breaks networking.",
        "symptomsd" => "Monitors network quality to help diagnose problems.",
        "trustdFileHelper" => "Helper for certificate checks.",
        "useractivityd" => "Handoff activity between your devices.",
        "pboard" => "The clipboard (copy and paste).",
        "universalaccessd" => "Accessibility features (zoom, VoiceOver support).",
        "coreduetd" | "knowledge-agent" | "duetexpertd" => "Learns your usage patterns to suggest apps and schedule background work.",
        "suggestd" => "Siri suggestions in apps and Spotlight.",
        "Siri" | "assistantd" | "siriknowledged" => "Siri.",
        "accountsd" => "Manages internet accounts (iCloud, Google, Microsoft) used by Mail, Calendar and others.",
        "secd" => "Syncs passwords and keychain items with iCloud Keychain.",
        "akd" => "Apple ID authentication.",
        "imagent" | "IMDPersistenceAgent" => "Messages and FaceTime background service.",
        "CommCenter" => "Phone and cellular features (calls via iPhone, SMS relay).",
        "callservicesd" => "Phone calls and FaceTime audio.",
        "locationd" => "Location services.",
        "timed" => "Keeps the system clock accurate.",
        "usbd" | "usbmuxd" => "USB devices and connections to iPhone/iPad.",
        "AMPDevicesAgent" | "AMPDeviceDiscoveryAgent" => "Finds and syncs iPhones and iPads connected to the Mac.",
        "lsd" => "Launch Services: knows which app opens which file type.",
        "iconservicesagent" | "iconservicesd" => "Loads and caches app and file icons.",
        "QuickLookUIService" | "quicklookd" => "Quick Look previews (press Space on a file).",
        "WindowManager" => "Stage Manager and window tiling.",
        "talagent" => "Restores app windows after a restart (Resume).",
        "screencaptureui" => "The screenshot and screen recording tool.",
        "TextInputMenuAgent" | "TextInputSwitcher" => "Keyboard input menu and input source switcher.",
        "AirPlayXPCHelper" | "AirPlayUIAgent" => "AirPlay streaming.",
        "storekitagent" | "storeassetd" | "storedownloadd" => "App Store purchases and downloads.",
        "remindd" => "Reminders sync.",
        "calaccessd" => "Calendar data access for apps.",
        "contactsd" => "Contacts data service.",
        "nfcd" => "NFC / Apple Pay hardware.",
        "biometrickitd" => "Touch ID.",
        "runningboardd" => "Decides how much CPU and memory each app may use.",
        "UserEventAgent" => "Handles system events (devices connecting, network changes) for macOS.",
        "corebrightnessd" => "Screen brightness and Night Shift.",
        "thermalmonitord" => "Monitors temperature to protect the hardware.",
        "spindump" => "Collects diagnostics when an app hangs.",
        "ReportCrash" => "Writes crash reports when an app crashes.",
        "zsh" | "bash" | "sh" | "fish" => "A command-line shell, usually running inside a Terminal window.",
        "login" => "Starts a shell session for a terminal window.",
        "ssh" => "A secure shell connection to another computer.",
        "sshd" => "Lets other computers connect to this Mac with SSH (Remote Login).",
        "node" => "Node.js: runs JavaScript programs (dev servers, tools, Electron helpers).",
        "python" | "python3" | "Python" => "A Python program.",
        "ruby" => "A Ruby program.",
        "java" => "A Java program.",
        "docker" | "com.docker.backend" | "com.docker.vmnetd" => "Docker containers.",
        "Code Helper" | "Code Helper (Renderer)" | "Code Helper (Plugin)" | "Code Helper (GPU)" => "Helper for Visual Studio Code.",
        "cargo" => "Rust's build tool.",
        "rustc" => "The Rust compiler.",
        "rust-analyzer" => "Rust code intelligence for your editor.",
        "git" => "Git version control.",
        _ => return None,
    })
}

pub struct Ctx<'a> {
    pub name: &'a str,
    pub exe: Option<&'a str>,
    pub kind: &'a str,
    pub app_name: Option<&'a str>,
    pub is_root: bool,
}

pub fn describe(c: &Ctx) -> String {
    if let Some(d) = known(c.name) {
        return d.to_string();
    }
    let lower = c.name.to_lowercase();
    let app = c.app_name.unwrap_or("its app");

    match c.kind {
        "app" => {
            return format!(
                "The app “{}”, open on your Mac.",
                c.app_name.unwrap_or(c.name)
            )
        }
        "menubar" => {
            return format!(
                "“{}” runs in the menu bar or in the background without a Dock icon.",
                c.app_name.unwrap_or(c.name)
            )
        }
        "helper" => {
            if lower.contains("renderer") {
                return format!("Renders web content (a tab, window or panel) for {app}.");
            }
            if lower.contains("gpu") {
                return format!("Handles graphics and video acceleration for {app}.");
            }
            if lower.contains("plugin") || lower.contains("extension") {
                return format!("Runs plugins or extensions for {app}.");
            }
            if lower.contains("helper") || lower.contains("agent") || lower.contains("service") {
                return format!("A helper process for {app}. Many apps split their work into helpers like this.");
            }
            if let Some(exe) = c.exe {
                if exe.contains(".app/") && !exe.starts_with("/System/") {
                    return format!("Part of {app}.");
                }
            }
            return format!("Started by {app}.");
        }
        _ => {}
    }

    if let Some(exe) = c.exe {
        if lower.starts_with("com.apple.") || exe.starts_with("/System/") {
            return "A built-in macOS component.".into();
        }
        if exe.starts_with("/usr/libexec/")
            || exe.starts_with("/usr/sbin/")
            || exe.starts_with("/sbin/")
        {
            return "A built-in macOS background service (daemon).".into();
        }
        if exe.starts_with("/usr/bin/") || exe.starts_with("/bin/") {
            return "A built-in macOS command-line tool.".into();
        }
        if exe.starts_with("/opt/homebrew/")
            || exe.starts_with("/usr/local/Cellar/")
            || exe.starts_with("/usr/local/opt/")
        {
            return "A command-line program installed with Homebrew.".into();
        }
        if exe.starts_with("/usr/local/") {
            return "A program installed in /usr/local, usually by a developer tool or installer."
                .into();
        }
        if let Some(vendor) = vendor_from_path(exe).or_else(|| vendor_from_reverse_dns(c.name)) {
            return format!("A background service installed by {vendor}.");
        }
        if exe.contains("/.cargo/") || exe.contains("/.rustup/") {
            return "A Rust development tool.".into();
        }
        if exe.contains("/node_modules/") || exe.contains("/.nvm/") {
            return "A Node.js development tool.".into();
        }
        if exe.contains(".app/") {
            if let Some(bundle) = outer_bundle_name(exe) {
                return format!("Part of the app “{bundle}”, running in the background.");
            }
        }
    }
    if let Some(vendor) = vendor_from_reverse_dns(c.name) {
        return format!("A background service installed by {vendor}.");
    }
    if c.is_root {
        return "A background service running as the system administrator (root).".into();
    }
    if c.name.ends_with('d') && c.name.len() > 2 && c.name.chars().all(|ch| ch.is_ascii_lowercase())
    {
        return "A background service (daemon). On macOS, names ending in “d” are usually daemons."
            .into();
    }
    "A background process. Use “Search the web” to find out more.".into()
}

/// "/Library/Application Support/Vendor/..." → "Vendor"
pub fn vendor_from_path(exe: &str) -> Option<String> {
    for prefix in [
        "/Library/Application Support/",
        "/Library/PrivilegedHelperTools/",
    ] {
        if let Some(i) = exe.find(prefix) {
            let rest = &exe[i + prefix.len()..];
            let first = rest.split('/').next()?;
            if !first.is_empty() {
                let first = first.trim_end_matches(".app");
                return Some(vendor_from_reverse_dns(first).unwrap_or_else(|| first.to_string()));
            }
        }
    }
    None
}

/// "com.wacom.DataStoreMgr" → "Wacom"; None if not reverse-DNS style.
pub fn vendor_from_reverse_dns(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split('.').collect();
    if parts.len() < 3
        || !matches!(
            parts[0],
            "com" | "io" | "org" | "net" | "co" | "dev" | "app"
        )
    {
        return None;
    }
    if parts[1] == "apple" {
        return None;
    }
    let v = parts[1];
    let mut ch = v.chars();
    let first = ch.next()?;
    Some(first.to_uppercase().collect::<String>() + ch.as_str())
}

/// Name of the outermost ".app" bundle in a path.
pub fn outer_bundle_name(exe: &str) -> Option<String> {
    exe.split('/')
        .find(|c| c.ends_with(".app"))
        .map(|c| c.trim_end_matches(".app").to_string())
}

/// Path of the outermost ".app" bundle in a path.
pub fn outer_bundle_path(exe: &str) -> Option<String> {
    let idx = exe.find(".app/")?;
    Some(exe[..idx + 4].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles() {
        let p = "/Applications/Google Chrome.app/Contents/Frameworks/X.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper";
        assert_eq!(outer_bundle_name(p).as_deref(), Some("Google Chrome"));
        assert_eq!(
            outer_bundle_path(p).as_deref(),
            Some("/Applications/Google Chrome.app")
        );
    }

    #[test]
    fn protected() {
        assert!(is_protected(1, "anything"));
        assert!(is_protected(500, "WindowServer"));
        assert!(!is_protected(500, "Safari"));
    }

    #[test]
    fn helper_descriptions() {
        let d = describe(&Ctx {
            name: "Google Chrome Helper (Renderer)",
            exe: None,
            kind: "helper",
            app_name: Some("Google Chrome"),
            is_root: false,
        });
        assert!(d.contains("Renders web content"));
        let d = describe(&Ctx {
            name: "foo",
            exe: Some("/Library/Application Support/Acme/foo"),
            kind: "service",
            app_name: None,
            is_root: false,
        });
        assert_eq!(d, "A background service installed by Acme.");
        let d = describe(&Ctx {
            name: "com.wacom.DataStoreMgr",
            exe: Some("/Library/Application Support/Tablet/com.wacom.DataStoreMgr.app/Contents/MacOS/com.wacom.DataStoreMgr"),
            kind: "service",
            app_name: None,
            is_root: false,
        });
        assert_eq!(d, "A background service installed by Tablet.");
        assert_eq!(
            vendor_from_reverse_dns("com.surfshark.vpnclient.macos").as_deref(),
            Some("Surfshark")
        );
        assert_eq!(vendor_from_reverse_dns("com.apple.foo"), None);
        assert_eq!(vendor_from_reverse_dns("postgres"), None);
    }
}
