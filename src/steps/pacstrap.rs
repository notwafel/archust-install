//! Step: multilib, pacstrap, fstab.

use crate::config::Config;
use crate::fs::Fs;
use crate::sys::{MNT, out, run as sh, write_file};
use crate::ui::{die, say};

const LIVE_CONF: &str = "/etc/pacman.conf";

pub fn base(cfg: &Config) {
    if cfg.multilib {
        say("multilib: uncommenting [multilib] in pacman.conf");
        // the target copy only exists after pacstrap, so only the live copy can be edited now;
        // it is copied into /mnt right after pacstrap
        let conf = std::fs::read_to_string(LIVE_CONF).unwrap_or_else(|_| die(&format!("cannot read {}", LIVE_CONF)));
        std::fs::write(LIVE_CONF, uncomment_multilib(&conf))
            .unwrap_or_else(|_| die(&format!("cannot write {}", LIVE_CONF)));
        sh("pacman", &["-Syy", "--noconfirm"]);
    }

    say("pacstrap");
    let mut args = vec!["-K", MNT];
    args.extend_from_slice(&cfg.packages);
    match cfg.fs {
        Fs::Xfs => args.push("xfsprogs"),
        Fs::Ext4 => args.push("e2fsprogs"),
        Fs::Btrfs => {}
    }
    sh("pacstrap", &args);

    if cfg.multilib {
        let live = std::fs::read_to_string(LIVE_CONF).unwrap_or_else(|_| die("cannot read live pacman.conf"));
        write_file("/etc/pacman.conf", &live);
    }

    say("fstab");
    let fstab = out("genfstab", &["-U", MNT]).unwrap_or_else(|| die("genfstab failed"));
    write_file("/etc/fstab", &fstab);
}

/// Uncomments the `[multilib]` repo block, keeping everything else untouched.
pub fn uncomment_multilib(conf: &str) -> String {
    let mut out = String::with_capacity(conf.len() + 64);
    let mut inside = false;
    for line in conf.lines() {
        let t = line.trim();
        if inside {
            if t.starts_with('[') {
                inside = false;
                out.push_str(line);
            } else {
                out.push_str(t.strip_prefix("# ").or_else(|| t.strip_prefix('#')).unwrap_or(t));
            }
        } else if t.starts_with('#') && t.contains("[multilib]") {
            inside = true;
            out.push_str("[multilib]");
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::uncomment_multilib;

    const IN: &str = "[core]\nInclude = /etc/pacman.d/mirrorlist\n\n#[multilib]\n#Include = /etc/pacman.d/mirrorlist\n\n[extra]\n#Include = keep\n";

    #[test]
    fn enables_multilib_only() {
        let out = uncomment_multilib(IN);
        assert!(out.contains("[multilib]\nInclude = /etc/pacman.d/mirrorlist"));
        assert!(out.contains("[extra]\n#Include = keep"));
    }

    #[test]
    fn idempotent() {
        let once = uncomment_multilib(IN);
        assert_eq!(once, uncomment_multilib(&once));
    }
}
