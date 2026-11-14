//! Disk inspection and partitioning: list disks, free space, existing ESP, mkpart.

use crate::layout::{Part, Role};
use crate::sys::{out, run};
use crate::ui::die;

/// The disk the running live ISO booted from; installing there would destroy the run itself.
pub fn is_live_disk(dev: &str) -> bool {
    let booted = out("lsblk", &["-no", "PKNAME", "/run/archiso/bootmnt"])
        .or_else(|| out("lsblk", &["-no", "PKNAME", "-J", "/dev/disk/by-label/ARCH"]))
        .unwrap_or_default();
    !booted.is_empty() && booted.lines().any(|l| l.trim() == dev.trim_start_matches("/dev/"))
}

/// `PARTED`-style listing for the summary before asking whether to keep it.
pub fn table(disk: &str) -> String {
    out("parted", &["-sm", disk, "unit", "MiB", "print"]).unwrap_or_default()
}

/// Unpartitioned space after the last partition, in MiB.
pub fn free_mib(disk: &str) -> u64 {
    let total = size_mib(disk);
    let used = free_start(disk);
    if used >= total { 0 } else { total - used + 1 }
}

pub fn dev_name(disk: &str, index: u32) -> String {
    if disk.ends_with(|c: char| c.is_ascii_digit()) {
        format!("{}p{}", disk, index)
    } else {
        format!("{}{}", disk, index)
    }
}

pub fn size_mib(dev: &str) -> u64 {
    out("blockdev", &["--getsize64", dev])
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or_else(|| die("cannot read disk size"))
        / 1048576
}

pub fn list() -> String {
    out("lsblk", &["-dpno", "NAME,SIZE,MODEL"])
        .unwrap_or_default()
        .lines()
        .map(|l| format!("  {}", l))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn describe(disk: &str) -> String {
    out("lsblk", &["-o", "NAME,SIZE,FSTYPE,LABEL,MOUNTPOINT", disk]).unwrap_or_default()
}

pub fn is_whole_disk(dev: &str) -> bool {
    dev.starts_with("/dev/")
        && crate::sys::exists(dev)
        && out("lsblk", &["-dpno", "TYPE", dev]).is_some_and(|t| t.trim() == "disk")
}

pub fn has_partitions(disk: &str) -> bool {
    out("lsblk", &["-no", "TYPE", disk])
        .is_some_and(|t| t.lines().any(|l| l.trim() == "part"))
}

/// Last used MiB + 2: one MiB alignment gap, parted ends are inclusive.
pub fn free_start(disk: &str) -> u64 {
    out("parted", &["-sm", disk, "unit", "MiB", "print"])
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("/dev/"))
        .filter_map(|l| l.split(':').nth(2))
        .filter_map(|e| e.trim_end_matches("MiB").parse::<f64>().ok())
        .fold(0f64, f64::max)
        .ceil() as u64
        + 2
}

pub fn existing_esp(disk: &str) -> Option<String> {
    out("lsblk", &["-pnlo", "NAME,PARTTYPENAME", disk])
        .and_then(|l| {
            l.lines()
                .find(|x| x.contains("EFI System"))
                .map(|x| x.split_whitespace().next().unwrap_or("").to_string())
        })
        .filter(|s| !s.is_empty() && crate::sys::exists(s))
}

fn partition_numbers(disk: &str) -> Vec<u32> {
    out("lsblk", &["-rno", "PARTN", disk])
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|l| l.parse().ok())
        .collect()
}

fn partition_count(disk: &str) -> u32 {
    partition_numbers(disk).len() as u32
}

/// Highest existing partition number, 0 when the disk is empty.
pub fn last_number(disk: &str) -> u32 {
    partition_numbers(disk).into_iter().max().unwrap_or(0)
}

/// Writes the plan to disk, returns the device path of each new partition.
pub fn create(disk: &str, parts: &[Part], dual: bool, root_fs_hint: &str) -> Vec<String> {
    if !dual {
        run("parted", &["-s", disk, "mklabel", "gpt"]);
    }
    let mut created = Vec::with_capacity(parts.len());
    for p in parts {
        let num = partition_count(disk) + 1;
        let (label, fs) = match p.role {
            Role::Esp => ("archust-efi", "fat32"),
            Role::Swap => ("archust-swap", "linux-swap"),
            // parted's fs hint only sets the GPT type; mkfs picks the real filesystem later
            Role::Root => ("archust-root", root_fs_hint),
        };
        run("parted", &[
            "-s", disk, "mkpart", label, fs,
            &format!("{}MiB", p.start),
            &format!("{}MiB", p.end + 1),
        ]);
        if p.role == Role::Esp {
            run("parted", &["-s", disk, "set", &num.to_string(), "esp", "on"]);
        }
        run("partprobe", &[disk]);
        let after = partition_count(disk);
        if after != num {
            die(&format!("expected {} partitions after mkpart, got {}", num, after));
        }
        created.push(dev_name(disk, num));
    }
    created
}
