#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let status =
        agentsassemble_desktop_lib::run_runtime_supervisor_if_requested().unwrap_or_else(|| {
            eprintln!("runtime supervisor requires its internal invocation");
            2
        });
    std::process::exit(status);
}
