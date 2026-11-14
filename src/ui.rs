//! Terminal output. Every user-facing line goes through here.

use std::process::exit;

const WIDTH: usize = 62;

pub fn say(m: &str) { println!("==> {}", m); }

pub fn warn(m: &str) { eprintln!("!!! {}", m); }

pub fn die(m: &str) -> ! {
    eprintln!("ERR {}", m);
    exit(1)
}

pub fn note(m: &str) { println!("{}", m); }

/// Section header: `[2/7] Partition layout`, underlined. Keeps the flow readable instead of a
/// wall of prompts.
pub fn step(index: usize, total: usize, title: &str) {
    println!();
    println!("[{}/{}] {}", index, total, title);
    println!("{}", "-".repeat(WIDTH));
}

pub fn rule() { println!("{}", "-".repeat(WIDTH)); }

/// `key: value` line inside a summary block.
pub fn item(key: &str, value: &str) {
    println!("  {:<14} {}", format!("{}:", key), value);
}
