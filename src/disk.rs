//! Disk inspection and partitioning: list disks, free space, existing ESP, mkpart.

use crate::layout::{Part, Role};
use crate::sys::{out, run};
use crate::ui::die;

/// Name of the disk the ISO boots from, e.g. `sdb`. Never offered in the picker.
fn boot_disk() -> Option<String> {
    // PKNAME is the whole disk behind the booted partition; a dd'd ISO has the label on a disk
    for dev in ["/run/archiso/bootmnt", "/dev/disk/by-label/ARCH"] {
        if let Some(pk) = out("lsblk", &["-no", "PKNAME", dev]).and_then(|s| first_word(&s)) {
            return Some(pk);
        }
        let row = out("lsblk", &["-dno", "TYPE,NAME", dev]).unwrap_or_default();
        if let Some(name) = row.strip_prefix("disk").and_then(first_word) {
            return Some(name);
        }
    }
    None
}

fn first_word(s: &str) -> Option<String> {
    s.split_whitespace().next().map(str::to_string)
}

/// Whole disks, ready for the `Select` picker. The live ISO's own disk is left out.
pub fn disks() -> Vec<String> {
    let live = boot_disk();
    out("lsblk", &["-dno", "NAME"])
        .unwrap_or_default()
        .lines()
        .map(|l| l.split_whitespace().next().unwrap_or("").to_string())
        .filter(|d| !d.is_empty() && live.as_deref() != Some(d.as_str()))
        .collect()
}

/// Raw `lsblk` rows: NAME, FSTYPE and mountpoints for the disk and its partitions.
fn listing(disk: &str) -> String {
    // -p: device paths, or `swapoff sda2` fails with "No such file or directory"
    out("lsblk", &["-lpno", "NAME,FSTYPE,MOUNTPOINTS", disk]).unwrap_or_default()
}

/// Splits an `lsblk` listing into active swap devices and mounted partitions. The first row is
/// the disk itself: no filesystem, no mountpoint, so it falls through both lists.
fn usage_from(listing: &str) -> (Vec<String>, Vec<String>) {
    let mut swap = Vec::new();
    let mut mounted = Vec::new();
    for line in listing.lines() {
        let mut fields = line.split_whitespace();
        let (Some(name), Some(fstype)) = (fields.next(), fields.next()) else { continue };
        let points = fields.collect::<Vec<_>>();
        if fstype == "swap" {
            swap.push(name.to_string());
        } else if !points.is_empty() {
            mounted.push(format!("{} on {}", name, points.join(",")));
        }
    }
    (swap, mounted)
}

/// Parted refuses to relabel a disk whose partitions are mounted or active as swap. Swap is
/// switched off (the live ISO may have picked it up); a mounted partition aborts the run.
fn clear_busy(disk: &str) {
    let (swap, mounted) = usage_from(&listing(disk));
    for dev in swap {
        run("swapoff", &[&dev]);
    }
    if !mounted.is_empty() {
        die(&format!("unmount first: {}", mounted.join(", ")));
    }
}

/// `PARTED`-style listing for the summary before asking whether to keep it.
pub fn table(disk: &str) -> String {
    out("parted", &["-sm", disk, "unit", "MiB", "print"]).unwrap_or_default()
}

/// Unpartitioned space after the last partition, in MiB, for a known disk size.
pub fn free_mib(disk: &str, total_mib: u64) -> u64 {
    let used = free_start(disk);
    if used >= total_mib { 0 } else { total_mib - used + 1 }
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

/// One picker line per disk: device, size with model, and what is already on it.
pub fn menu(names: &[String]) -> Vec<String> {
    names
        .iter()
        .map(|name| {
            let dev = format!("/dev/{}", name);
            let details = out("lsblk", &["-dpno", "SIZE,MODEL", &dev])
                .unwrap_or_default()
                .trim()
                .to_string();
            let listing = listing(&dev);
            let parts = listing.lines().skip(1).count();
            let (_, mounted) = usage_from(&listing);
            let state = if parts == 0 {
                "empty".to_string()
            } else if mounted.is_empty() {
                format!("{} partition{}", parts, if parts == 1 { "" } else { "s" })
            } else {
                format!("{} partitions, in use", parts)
            };
            format!("{:<13} {:<28} {}", dev, details, state)
        })
        .collect()
}

pub fn describe(disk: &str) -> String {
    out("lsblk", &["-o", "NAME,SIZE,FSTYPE,LABEL,MOUNTPOINT", disk]).unwrap_or_default()
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

/// Filesystem blkid sees on a device; empty when it sees none (unformatted or wiped).
pub fn fs_type(dev: &str) -> String {
    out("blkid", &["-s", "TYPE", "-o", "value", dev])
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn partition_numbers(disk: &str) -> Vec<u32> {
    partition_numbers_from_table(&table(disk))
}

fn partition_numbers_from_table(table: &str) -> Vec<u32> {
    table.lines()
        .filter_map(|line| line.split(':').next()?.parse().ok())
        .collect()
}

/// Writes the plan to disk, returns the device path of each new partition.
pub fn create(disk: &str, parts: &[Part], dual: bool, root_fs_hint: &str) -> Vec<String> {
    if !dual {
        // the table is about to go; parted refuses a disk with mounted partitions or active swap
        clear_busy(disk);
        run("parted", &["-s", disk, "mklabel", "gpt"]);
    }
    let mut created = Vec::with_capacity(parts.len());
    for p in parts {
        let before = partition_numbers(disk);
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
        run("partprobe", &[disk]);
        run("udevadm", &["settle"]);
        let added = partition_numbers(disk)
            .into_iter()
            .filter(|number| !before.contains(number))
            .collect::<Vec<_>>();
        if added.len() != 1 {
            die(&format!("expected one new partition after mkpart, got {}", added.len()));
        }
        let num = added[0];
        if p.role == Role::Esp {
            run("parted", &["-s", disk, "set", &num.to_string(), "esp", "on"]);
            run("partprobe", &[disk]);
            run("udevadm", &["settle"]);
        }
        created.push(dev_name(disk, num));
    }
    created
}

/// Swap device and hibernation flag, read from the partitions that were just created.
pub fn swap_device(parts: &[Part], devices: &[String], hibernate: bool) -> Option<(String, bool)> {
    parts.iter().position(|p| p.role == Role::Swap).map(|i| (devices[i].clone(), hibernate))
}

#[cfg(test)]
mod tests {
    use super::{partition_numbers_from_table, usage_from};

    #[test]
    fn reads_partition_numbers_from_parted_machine_output() {
        let table = "BYT;\n/dev/sda:1024MiB:scsi:512:512:gpt:;\n1:1MiB:513MiB:512MiB:fat32:archust-efi:esp;\n3:514MiB:1023MiB:510MiB:ext4:archust-root:;\n";
        assert_eq!(partition_numbers_from_table(table), [1, 3]);
    }

    #[test]
    fn finds_mounted_and_swap_partitions() {
        let listing = "/dev/sda\n/dev/sda1 vfat /boot\n/dev/sda2 swap [SWAP]\n/dev/sda3 ext4 /mnt\n/dev/sda4\n";
        let (swap, mounted) = usage_from(listing);
        assert_eq!(swap, ["/dev/sda2"]);
        assert_eq!(mounted, ["/dev/sda1 on /boot", "/dev/sda3 on /mnt"]);
    }
}
