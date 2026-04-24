// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "desktop")]
fn main() {
    std::process::exit(recupere_lib::run_entrypoint())
}

// Headless build (e.g. when only the engine is compiled for the agent crate):
// the desktop binary is not produced, but cargo still requires a `main`.
#[cfg(not(feature = "desktop"))]
fn main() {}
