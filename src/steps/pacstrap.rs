//! Step: multilib, pacstrap, fstab.

use crate::config::Config;
use crate::fs::Fs;
use crate::sys::{MNT, out, run as sh, write_file};
use crate::ui::{die, say};

const LIVE_CONF: &str = "/etc/pacman.conf";

pub fn base(cfg: &Config, swap: &Option<(String, bool)>) {
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
    let mut args: Vec<&str> = vec!["-K", MNT];
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
        // pacstrap -C or a failed copy would leave the target without [multilib]
        let stuck = std::fs::read_to_string(format!("{}{}", MNT, LIVE_CONF)).unwrap_or_default();
        if uncomment_multilib(&stuck) != stuck {
            die("multilib is not enabled in the target /etc/pacman.conf");
        }
    }

    say("fstab");
    let fstab = out("genfstab", &["-U", MNT]).unwrap_or_else(|| die("genfstab failed"));
    write_file("/etc/fstab", &fstab);

    // genfstab only sees a formatted swap that is active; `swapon` must come before it
    if let Some((sw, hibernate)) = swap {
        write_file("/etc/mkinitcpio.conf.d/10-resume-device.conf",
            &format!("RESUME=UUID={}\n", crate::steps::filesystems::root_uuid(sw)));
        if *hibernate {
            write_file("/etc/default/grub.d/10-resume.cfg",
                &format!("GRUB_CMDLINE_LINUX_DEFAULT=\"${{GRUB_CMDLINE_LINUX_DEFAULT}} resume=UUID={}\"\n",
                    crate::steps::filesystems::root_uuid(sw)));
        }
    }
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
