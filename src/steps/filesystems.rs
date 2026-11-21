//! Step: filesystems (mkfs) and mounts. Both depend on the chosen `Fs`.

use crate::config::Config;
use crate::fs::{self, Fs};
use crate::layout::Role;
use crate::sys::{MNT, out, run as sh};
use crate::ui::{die, note, say};
use std::fs as stdfs;

pub struct Formatted {
    pub esp: String,
    pub root_raw: String,
}

pub fn format(cfg: &Config, devices: &[String]) -> Formatted {
    say("Filesystems");
    // an existing ESP chosen for formatting is erased only now, after the final confirmation
    if cfg.format_esp {
        let esp = cfg.esp_part.as_deref().unwrap_or_else(|| die("no ESP to format"));
        // mkfs.fat happily overwrites a mounted ESP; stop instead of pulling the rug out
        crate::disk::clear_busy(esp);
        sh("mkfs.fat", &["-F32", "-n", "ESP", esp]);
    }
    let mut esp = cfg.esp_part.clone().unwrap_or_default();
    let mut root_raw = String::new();
    for (i, part) in cfg.parts.iter().enumerate() {
        let dev = &devices[i];
        match part.role {
            Role::Esp => {
                sh("mkfs.fat", &["-F32", "-n", "ESP", dev]);
                esp = dev.clone();
            }
            Role::Swap => {
                sh("mkswap", &["-L", "swap", dev]);
                // genfstab runs before mount(), so the swap has to be active now or it is
                // missing from the target's fstab and hibernation has nothing to resume from
                sh("swapon", &[dev]);
            }
            Role::Root => root_raw = dev.clone(),
        }
    }
    if root_raw.is_empty() {
        die("no root partition in plan");
    }
    note(&format!("  {} {}{}", cfg.fs, root_raw, if cfg.luks { " (LUKS)" } else { "" }));
    Formatted { esp, root_raw }
}

pub fn mount(cfg: &Config, fs: &Formatted, root: &str) {
    say("Mounts");
    let opts = cfg.fs.fstab_options();
    // format first: a mount of the ESP below would sit on the live /mnt and be hidden by the
    // new root mounted over it, leaving /boot empty on the installed system
    cfg.fs.mkfs(root);
    // -t keeps mount from probing every type; the probe noise ("EXT4-fs: Can't find ext4
    // filesystem") reads like a real failure in the log
    let kind = cfg.fs.key();
    match cfg.fs {
        Fs::Btrfs => {
            sh("mount", &["-t", kind, root, MNT]);
            for name in fs::SUBVOLS {
                sh("btrfs", &["subvolume", "create", &format!("{}/{}", MNT, name)]);
            }
            sh("umount", &[MNT]);
            sh("mount", &["-t", kind, "-o", &format!("subvol=@,{}", opts), root, MNT]);
            for (name, path) in fs::SUBVOLS.iter().zip(fs::SUBVOL_TARGETS.iter()).skip(1) {
                let target = format!("{}{}", MNT, path);
                stdfs::create_dir_all(&target).ok();
                sh("mount", &["-t", kind, "-o", &format!("subvol={},{}", name, opts), root, &target]);
            }
        }
        _ => sh("mount", &["-t", kind, "-o", opts, root, MNT]),
    }
    // grub-mkconfig probes /boot; without this it may see the live ISO's own fstab entries
    stdfs::create_dir_all(format!("{}/boot", MNT)).ok();
    // -t vfat fails with a clear message when the module is missing; without it mount probes
    // ext2/ext3 first and only then reports "unknown filesystem type 'vfat'"
    if !crate::sys::modprobe("vfat") {
        die("vfat kernel module missing for the running kernel - reboot the live ISO, do not pacman -Syu it");
    }
    sh("mount", &["-t", "vfat", &fs.esp, &format!("{}/boot", MNT)]);
}

/// UUID of the root filesystem (LUKS mapper when encrypted).
pub fn root_uuid(root: &str) -> String {
    out("blkid", &["-s", "UUID", "-o", "value", root])
        .unwrap_or_else(|| die("cannot read root UUID"))
        .trim()
        .to_string()
}
