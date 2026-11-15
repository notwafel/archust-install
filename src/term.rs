//! Terminal prompts, archinstall-style: arrow keys, Enter to accept, Esc/Ctrl-C to abort.
//!
//! Nothing exits on a bad answer - `Input` re-asks via its validator, menus always yield a value.

use crate::ui::{die, warn};
use dialoguer::{Confirm, FuzzySelect, Input, Password, Select};

fn finish<T>(r: dialoguer::Result<T>) -> T {
    r.unwrap_or_else(|e| die(&format!("input aborted: {}", e)))
}

pub fn ask(name: &str) -> String {
    finish(
        Input::<String>::new()
            .with_prompt(name)
            .validate_with(|s: &String| -> Result<(), &str> {
                let s = s.trim();
                if s.is_empty() {
                    Err("required")
                } else if !s.starts_with("/dev/") {
                    Err("must be a path like /dev/sda")
                } else {
                    Ok(())
                }
            })
            .interact_text(),
    )
    .trim()
    .to_string()
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

/// Searchable menu. `Esc` returns `None`, so the caller can reopen the previous field.
pub fn pick_fuzzy<T: Clone + std::fmt::Display>(name: &str, items: &[T], default: usize) -> Option<T> {
    if items.is_empty() {
        die(&format!("{}: nothing to choose from", name));
    }
    let labels: Vec<String> = items.iter().map(|i| i.to_string()).collect();
    finish(FuzzySelect::new().with_prompt(name).items(&labels).default(default).interact_opt())
        .map(|idx| items[idx].clone())
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

/// Picks one disk from `lsblk`; returns its name, e.g. `sda` or `nvme0n1`.
pub fn pick_disk(disks: &[String]) -> usize {
    let labels: Vec<String> = disks.iter().map(|d| format!("/dev/{}", d)).collect();
    if labels.is_empty() {
        die("no disks found");
    }
    finish(Select::new().with_prompt("Install disk").items(&labels).default(0).interact())
}

/// Hidden input, asked twice; empty, short or mismatched just asks again.
pub fn ask_pass(name: &str) -> String {
    loop {
        let a = finish(Password::new().with_prompt(name).interact());
        if a.len() < 8 {
            warn("password needs at least 8 characters, type it again");
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
