#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(status) = agentsassemble_desktop_lib::run_runtime_supervisor_if_requested() {
        std::process::exit(status);
    }
    agentsassemble_desktop_lib::run();
}
