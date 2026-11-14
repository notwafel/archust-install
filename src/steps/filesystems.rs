//! Step: filesystems (mkfs) and mounts. Both depend on the chosen `Fs`.

use crate::config::Config;
use crate::fs::{self, Fs};
use crate::layout::Role;
use crate::sys::{MNT, out, run as sh};
use crate::ui::{die, note, say};
use std::fs as stdfs;

pub struct Formatted {
    pub esp: String,
    pub swaps: Vec<String>,
    pub root_raw: String,
}

pub fn format(cfg: &Config, devices: &[String]) -> Formatted {
    say("Filesystems");
    let mut esp = cfg.esp_part.clone().unwrap_or_default();
    let mut swaps = Vec::new();
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
                swaps.push(dev.clone());
            }
            Role::Root => root_raw = dev.clone(),
        }
    }
    if root_raw.is_empty() {
        die("no root partition in plan");
    }
    note(&format!("  {} {}{}", cfg.fs, root_raw, if cfg.luks { " (LUKS)" } else { "" }));
    Formatted { esp, swaps, root_raw }
}

pub fn mount(cfg: &Config, fs: &Formatted, root: &str) {
    say("Mounts");
    let opts = cfg.fs.fstab_options();
    match cfg.fs {
        Fs::Btrfs => {
            sh("mkfs.btrfs", &["-f", "-L", "archust", root]);
            sh("mount", &["-o", &format!("subvol=@,{}", opts), root, MNT]);
            for (name, path) in fs::SUBVOLS.iter().zip(fs::SUBVOL_TARGETS.iter()).skip(1) {
                sh("btrfs", &["subvolume", "create", &format!("{}/{}", MNT, name)]);
                let target = format!("{}{}", MNT, path);
                stdfs::create_dir_all(&target).ok();
                sh("mount", &["-o", &format!("subvol={},{}", name, opts), root, &target]);
            }
        }
        other => {
            other.mkfs(root);
            sh("mount", &["-o", opts, root, MNT]);
        }
    }
    stdfs::create_dir_all(format!("{}/boot", MNT)).ok();
    sh("mount", &[&fs.esp, &format!("{}/boot", MNT)]);
    for sw in &fs.swaps {
        sh("swapon", &[sw]);
    }
}

/// UUID of the root filesystem (LUKS mapper when encrypted).
pub fn root_uuid(root: &str) -> String {
    out("blkid", &["-s", "UUID", "-o", "value", root])
        .unwrap_or_else(|| die("cannot read root UUID"))
        .trim()
        .to_string()
}
