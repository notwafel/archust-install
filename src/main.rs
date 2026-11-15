//! Archust Install - Arch installer for the live ISO. Plain Arch, no archinstall.

mod config;
mod detect;
mod disk;
mod fs;
mod layout;
mod prompt;
mod steps;
mod sys;
mod term;
mod ui;

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--check") => std::process::exit(if layout::self_check() { 0 } else { 1 }),
        Some("-h") | Some("--help") => {
            println!("Archust Install - Arch installer for the live ISO\n\n  archust-install           install\n  archust-install --check   layout self-test\n  archust-install --help");
        }
        Some(x) => ui::die(&format!("unknown argument: {}", x)),
        None => run(),
    }
}

fn run() {
    sys::require_live_iso();
    sys::prepare_mirrors();
    let cfg = prompt::ask_everything();
    sys::check_packages(&cfg.packages, cfg.multilib);
    ui::say(&format!("Packages ({}): {}", cfg.packages.len(), cfg.packages.join(" ")));
    if !term::ask_yes("Start the installation?", false) {
        ui::die("aborted");
    }
    steps::install(&cfg);
    ui::say(&format!("Done. Unmount and reboot, then log in as {}.", cfg.user));
}
