// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|a| a == "--dump-startup") {
        return process_app_lib::dump_startup();
    }
    if std::env::args().any(|a| a == "--dump") {
        return process_app_lib::dump();
    }
    process_app_lib::run()
}
