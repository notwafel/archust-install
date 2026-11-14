//! Terminal prompts, archinstall-style: arrow keys, Enter to accept, Esc/Ctrl-C to abort.
//!
//! Nothing exits on a bad answer - `Input` re-asks via its validator, menus always yield a value.

use crate::ui::{die, warn};
use dialoguer::{Confirm, Input, Password, Select};

fn finish<T>(r: dialoguer::Result<T>) -> T {
    r.unwrap_or_else(|e| die(&format!("input aborted: {}", e)))
}

pub fn ask(name: &str, default: &str) -> String {
    finish(
        Input::<String>::new()
            .with_prompt(name)
            .default(default.to_string())
            .validate_with(|s: &String| -> Result<(), &str> {
                if s.trim().is_empty() { Err("cannot be empty") } else { Ok(()) }
            })
            .interact_text(),
    )
}

/// Lowercase letters, digits, `-` and `_`, 1-31 chars: valid both as a hostname and as a
/// username (`useradd` and `hostnamectl` reject the rest).
pub fn ask_name(name: &str) -> String {
    finish(
        Input::<String>::new()
            .with_prompt(name)
            .validate_with(|s: &String| -> Result<(), &str> {
                let s = s.trim();
                if s.is_empty() {
                    return Err("required");
                }
                if s.len() > 31 {
                    return Err("at most 31 characters");
                }
                if !s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
                    return Err("lowercase letters, digits, - and _ only");
                }
                if s.starts_with(|c: char| c.is_ascii_digit()) {
                    return Err("cannot start with a digit");
                }
                Ok(())
            })
            .interact_text(),
    )
    .trim()
    .to_string()
}

pub fn ask_num(name: &str, default: u64) -> u64 {
    finish(Input::<u64>::new().with_prompt(name).default(default).interact_text())
}

pub fn ask_num_min(name: &str, default: u64, min: u64) -> u64 {
    finish(
        Input::<u64>::new()
            .with_prompt(name)
            .default(default)
            .validate_with(move |v: &u64| {
                if *v < min { Err(format!("minimum is {}", min)) } else { Ok(()) }
            })
            .interact_text(),
    )
}

pub fn ask_yes(name: &str, default_yes: bool) -> bool {
    finish(Confirm::new().with_prompt(name).default(default_yes).interact())
}

/// Arrow-key menu; returns the chosen item.
pub fn pick<T: Clone + std::fmt::Display>(name: &str, items: &[T], default: usize) -> T {
    if items.is_empty() {
        die(&format!("{}: nothing to choose from", name));
    }
    let labels: Vec<String> = items.iter().map(|i| i.to_string()).collect();
    let idx = finish(Select::new().with_prompt(name).items(&labels).default(default).interact());
    items[idx].clone()
}

/// Multi-choice, `[ ]`/`[x]`; space toggles, Enter accepts.
pub fn pick_many<T: Clone + std::fmt::Display>(name: &str, items: &[T], checked: &[bool]) -> Vec<T> {
    let labels: Vec<String> = items.iter().map(|i| i.to_string()).collect();
    let defaults: Vec<bool> = checked.to_vec();
    let picks = finish(
        dialoguer::MultiSelect::new()
            .with_prompt(name)
            .items(&labels)
            .defaults(&defaults)
            .interact(),
    );
    picks.into_iter().map(|i| items[i].clone()).collect()
}

/// Hidden input, asked twice; mismatch or empty just asks again.
pub fn ask_pass(name: &str) -> String {
    loop {
        let a = finish(Password::new().with_prompt(name).interact());
        if a.is_empty() {
            warn("password cannot be empty, type it again");
            continue;
        }
        let b = finish(Password::new().with_prompt("repeat").interact());
        if a != b {
            warn("passwords differ, type again");
            continue;
        }
        return a;
    }
}
