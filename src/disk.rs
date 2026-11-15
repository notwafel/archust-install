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
    disk_names(&out("lsblk", &["-dno", "TYPE,NAME"]).unwrap_or_default())
        .into_iter()
        .filter(|d| live.as_deref() != Some(d.as_str()))
        .collect()
}

/// Keeps `disk` rows only: the live ISO also has `loop` (squashfs) and sometimes `rom` devices,
/// and none of them can hold an installation.
fn disk_names(rows: &str) -> Vec<String> {
    rows.lines()
        .filter_map(|row| {
            let mut fields = row.split_whitespace();
            (fields.next() == Some("disk"))
                .then(|| fields.next().map(str::to_string))
                .flatten()
        })
        .collect()
}

/// Raw `lsblk` rows: NAME, TYPE, FSTYPE and mountpoints for the disk and its children.
fn listing(disk: &str) -> String {
    // -p: device paths, or `swapoff sda2` fails with "No such file or directory";
    // -P: KEY="value" pairs, so a mountpoint with spaces stays one field
    out("lsblk", &["-lpno", "NAME,TYPE,FSTYPE,MOUNTPOINTS", "-P", disk]).unwrap_or_default()
}

/// One `lsblk -P` row.
struct Row {
    name: String,
    kind: String,
    fstype: String,
    mounts: String,
}

fn rows(listing: &str) -> Vec<Row> {
    listing
        .lines()
        .map(|line| Row {
            name: value(line, "NAME").unwrap_or_default().to_string(),
            kind: value(line, "TYPE").unwrap_or_default().to_string(),
            fstype: value(line, "FSTYPE").unwrap_or_default().to_string(),
            mounts: value(line, "MOUNTPOINTS").unwrap_or_default().replace('\n', ", "),
        })
        .collect()
}

/// Reads one `KEY="value"` pair from an `lsblk -P` row.
fn value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let mut rest = line;
    loop {
        let (head, after) = rest.split_once('"')?;
        let (val, tail) = after.split_once('"')?;
        if head.trim().trim_end_matches('=') == key {
            return Some(val);
        }
        rest = tail;
    }
}

#[derive(Default)]
struct Usage {
    swap: Vec<String>,
    mounted: Vec<String>,
    crypt: Vec<String>,
}

/// Active swap, mounted children and open LUKS mappers below the disk.
fn usage_from(listing: &str) -> Usage {
    let mut usage = Usage::default();
    for row in rows(listing) {
        if row.kind == "crypt" {
            usage.crypt.push(row.name.clone());
        }
        if row.fstype == "swap" {
            // `[SWAP]` is the mountpoint of an active swap; a formatted but unused swap
            // partition has no mountpoint and `swapoff` would fail on it
            if row.mounts.contains("[SWAP]") {
                usage.swap.push(row.name);
            }
        } else if !row.mounts.is_empty() {
            usage.mounted.push(format!("{} on {}", row.name, row.mounts));
        }
    }
    usage
}

/// Counts `part` rows only; a LUKS mapper is a `crypt` child of a partition.
fn partition_count(listing: &str) -> usize {
    rows(listing).iter().filter(|row| row.kind == "part").count()
}

/// Parted refuses to relabel a disk whose partitions are mounted, active as swap or held by an
/// open LUKS mapper. Swap is switched off; the rest aborts with the manual recovery command.
fn clear_busy(disk: &str) {
    let usage = usage_from(&listing(disk));
    for dev in &usage.swap {
        run("swapoff", &[dev.as_str()]);
    }
    if !usage.crypt.is_empty() {
        let close = usage
            .crypt
            .iter()
            .map(|mapper| format!("cryptsetup close {}", mapper.trim_start_matches("/dev/mapper/")))
            .collect::<Vec<_>>()
            .join(", ");
        die(&format!("close the open LUKS mapper first: {}", close));
    }
    if !usage.mounted.is_empty() {
        die(&format!("unmount first: {}", usage.mounted.join(", ")));
    }
}

/// `PARTED`-style listing for the summary before asking whether to keep it.
pub fn table(disk: &str) -> String {
    out("parted", &["-sm", disk, "unit", "MiB", "print"]).unwrap_or_default()
}

/// Unpartitioned space after the last partition, in MiB, for a known disk size.
pub fn free_mib_from_table(table: &str, total_mib: u64) -> u64 {
    // the last usable MiB is total - 2, the backup GPT needs the one after it
    total_mib.saturating_sub(last_end_from_table(table)).saturating_sub(1)
}

/// What step 2 does with a disk that already has partitions.
#[derive(Debug, PartialEq)]
pub enum EspChoice {
    /// Enough free space and a FAT32 ESP: reuse both.
    Reuse(u64),
    /// Enough free space, but the ESP needs a fresh FAT32 filesystem.
    Format(u64),
    /// Free space after the last partition, which is below the 16 GiB dualboot minimum.
    TooSmall(u64),
    /// No EFI System partition to reuse.
    NoEsp,
}

/// Pure keep/wipe decision; `table` is `parted -sm` output for `total_mib`.
pub fn keep_choice(table: &str, total_mib: u64, esp: Option<&str>, fstype: &str) -> EspChoice {
    let free = free_mib_from_table(table, total_mib);
    match esp {
        None => EspChoice::NoEsp,
        Some(_) if free < 16384 => EspChoice::TooSmall(free),
        Some(_) if fstype != "vfat" => EspChoice::Format(free),
        Some(_) => EspChoice::Reuse(free),
    }
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
            // a LUKS mapper is a `crypt` child, not another partition
            let parts = partition_count(&listing);
            let in_use = rows(&listing).iter().any(|row| !row.mounts.is_empty());
            let state = if parts == 0 {
                "empty".to_string()
            } else if !in_use {
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
    let table = out("parted", &["-sm", disk, "unit", "MiB", "print"])
        .unwrap_or_else(|| die("cannot read the partition table"));
    last_end_from_table(&table) + 2
}

/// `parted -sm` partition rows start with the partition number; the disk row starts with `/dev/`.
fn last_end_from_table(table: &str) -> u64 {
    table.lines()
        .filter(|line| line.split(':').next().is_some_and(|n| n.parse::<u32>().is_ok()))
        .filter_map(|line| line.split(':').nth(2))
        .filter_map(|end| end.trim_end_matches("MiB").parse::<f64>().ok())
        .fold(0f64, f64::max)
        .ceil() as u64
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
    use super::{
        EspChoice, disk_names, keep_choice, last_end_from_table, partition_count,
        partition_numbers_from_table, usage_from,
    };

    #[test]
    fn reads_partition_numbers_from_parted_machine_output() {
        let table = "BYT;\n/dev/sda:1024MiB:scsi:512:512:gpt:;\n1:1MiB:513MiB:512MiB:fat32:archust-efi:esp;\n3:514MiB:1023MiB:510MiB:ext4:archust-root:;\n";
        assert_eq!(partition_numbers_from_table(table), [1, 3]);
    }

    #[test]
    fn finds_mounted_and_swap_partitions() {
        let listing = "NAME=\"/dev/sda\" TYPE=\"disk\" FSTYPE=\"\" MOUNTPOINTS=\"\"\n\
                       NAME=\"/dev/sda1\" TYPE=\"part\" FSTYPE=\"vfat\" MOUNTPOINTS=\"/boot\"\n\
                       NAME=\"/dev/sda2\" TYPE=\"part\" FSTYPE=\"swap\" MOUNTPOINTS=\"[SWAP]\"\n\
                       NAME=\"/dev/sda3\" TYPE=\"part\" FSTYPE=\"ext4\" MOUNTPOINTS=\"/mnt\"\n\
                       NAME=\"/dev/sda4\" TYPE=\"part\" FSTYPE=\"swap\" MOUNTPOINTS=\"\"\n";
        let usage = usage_from(listing);
        assert_eq!(usage.swap, ["/dev/sda2"]);
        assert_eq!(usage.mounted, ["/dev/sda1 on /boot", "/dev/sda3 on /mnt"]);
        assert!(usage.crypt.is_empty());
    }

    #[test]
    fn mountpoints_with_spaces_stay_one_field() {
        let listing = "NAME=\"/dev/sdb1\" TYPE=\"part\" FSTYPE=\"exfat\" MOUNTPOINTS=\"/run/media/user/My Disk\"\n";
        assert_eq!(usage_from(listing).mounted, ["/dev/sdb1 on /run/media/user/My Disk"]);
    }

    #[test]
    fn open_luks_mapper_is_not_a_partition() {
        let listing = "NAME=\"/dev/sda1\" TYPE=\"part\" FSTYPE=\"crypto_LUKS\" MOUNTPOINTS=\"\"\n\
                       NAME=\"/dev/mapper/root\" TYPE=\"crypt\" FSTYPE=\"ext4\" MOUNTPOINTS=\"\"\n";
        assert_eq!(partition_count(listing), 1);
        assert_eq!(usage_from(listing).crypt, ["/dev/mapper/root"]);
    }

    #[test]
    fn install_disks_skip_loop_and_rom_devices() {
        let rows = "loop loop0\nrom sr0\ndisk sda\ndisk nvme0n1\n";
        assert_eq!(disk_names(rows), ["sda", "nvme0n1"]);
    }

    #[test]
    fn free_start_follows_the_last_partition() {
        // parted machine output: the disk row starts with /dev/, partition rows with a number
        let table = "BYT;\n/dev/sda:204800MiB:scsi:512:512:gpt:;\n1:1MiB:513MiB:512MiB:fat32:archust-efi:esp;\n3:514MiB:1023MiB:510MiB:ext4:archust-root:;\n";
        assert_eq!(last_end_from_table(table), 1023);
    }

    #[test]
    fn keep_choice_reuses_only_a_fat_esp_with_enough_room() {
        let table = "BYT;\n/dev/sda:204800MiB:scsi:512:512:gpt:;\n1:1MiB:1023MiB:1022MiB:ext4:old-root:;\n";
        let free = 204800 - 1023 - 1;
        assert_eq!(keep_choice(table, 204800, Some("/dev/sda1"), "vfat"), EspChoice::Reuse(free));
        assert_eq!(keep_choice(table, 204800, Some("/dev/sda1"), "ext4"), EspChoice::Format(free));
        assert_eq!(keep_choice(table, 204800, None, ""), EspChoice::NoEsp);

        let big = "BYT;\n/dev/sda:204800MiB:scsi:512:512:gpt:;\n1:1MiB:190000MiB:189999MiB:ext4:old-root:;\n";
        assert_eq!(keep_choice(big, 204800, Some("/dev/sda1"), "vfat"), EspChoice::TooSmall(14799));
    }
}
