//! Step: multilib, pacstrap, fstab.

use crate::config::Config;
use crate::fs::Fs;
use crate::sys::{MNT, out, run as sh, write_file};
use crate::ui::{die, say};

const LIVE_CONF: &str = "/etc/pacman.conf";

/// Enables `[multilib]` in the live config. The precheck and `base` both call it; the second
/// call is a no-op.
pub fn enable_multilib() {
    let conf = std::fs::read_to_string(LIVE_CONF).unwrap_or_else(|_| die(&format!("cannot read {}", LIVE_CONF)));
    let enabled = uncomment_multilib(&conf);
    if enabled == conf {
        return;
    }
    say("multilib: uncommenting [multilib] in pacman.conf");
    std::fs::write(LIVE_CONF, enabled)
        .unwrap_or_else(|_| die(&format!("cannot write {}", LIVE_CONF)));
    sh("pacman", &["-Syy", "--noconfirm"]);
}

pub fn base(cfg: &Config, swap: &Option<(String, bool)>) {
    if cfg.multilib {
        // the target copy only exists after pacstrap, so only the live copy can be edited now;
        // it is copied into /mnt right after pacstrap
        enable_multilib();
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
    // the current Arch resume hook reads `resume=` from the kernel command line, not RESUME=
    if let Some((sw, true)) = swap {
        write_file("/etc/default/grub.d/10-resume.cfg",
            &format!("GRUB_CMDLINE_LINUX_DEFAULT=\"${{GRUB_CMDLINE_LINUX_DEFAULT}} resume=UUID={}\"\n",
                crate::steps::filesystems::root_uuid(sw)));
    }
}

/// Uncomments the `[multilib]` repo block, keeping everything else untouched. The block ends at
/// the next header, commented or not, and only repo directives lose their `#`.
pub fn uncomment_multilib(conf: &str) -> String {
    const DIRECTIVES: [&str; 5] = ["Include", "Server", "SigLevel", "Usage", "CacheServer"];
    let mut out = String::with_capacity(conf.len() + 64);
    let mut inside = false;
    for line in conf.lines() {
        let t = line.trim();
        let header = t.trim_start_matches('#').trim();
        if header.starts_with('[') {
            inside = header == "[multilib]";
            out.push_str(if inside { "[multilib]" } else { line });
        } else if inside && t.starts_with('#') {
            let body = t.trim_start_matches('#').trim_start();
            // only `Key = value` directives; a comment like `#Include extra repos` must stay
            let directive = body.split_once('=').is_some_and(|(key, _)| DIRECTIVES.contains(&key.trim()));
            if directive {
                out.push_str(body);
            } else {
                out.push_str(line);
            }
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

    #[test]
    fn leaves_later_commented_sections_alone() {
        // custom ISOs append a commented [custom] repo after [multilib]; uncommenting it makes
        // pacman fetch file:///home/custompkgs/custom.db and fail every sync
        const ISO: &str = "[core]\nInclude = /etc/pacman.d/mirrorlist\n\n#[multilib]\n#Include = /etc/pacman.d/mirrorlist\n## Include extra repos\n\n## do not touch\n\n#[multilib-testing]\n#Include = /etc/pacman.d/mirrorlist\n\n#[custom]\n#SigLevel = Optional TrustAll\n#Server = file:///home/custompkgs\n";
        let out = uncomment_multilib(ISO);
        assert!(out.contains("[multilib]\nInclude = /etc/pacman.d/mirrorlist"));
        assert!(out.contains("## Include extra repos"));
        assert!(out.contains("## do not touch"));
        assert!(out.contains("#[multilib-testing]"));
        assert!(out.contains("#[custom]\n#SigLevel = Optional TrustAll\n#Server = file:///home/custompkgs"));
    }
}
