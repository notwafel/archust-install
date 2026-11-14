//! Terminal output. Every user-facing line goes through here.

use std::process::exit;

pub fn say(m: &str) { println!("==> {}", m); }

pub fn warn(m: &str) { eprintln!("!!! {}", m); }

pub fn die(m: &str) -> ! {
    eprintln!("ERR {}", m);
    exit(1)
}

pub fn note(m: &str) { println!("{}", m); }
