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
        // -d prints one row; a dd'd ISO is `disk sdb`, never a `loop`/`rom` device
        if let Some(name) = row.split_whitespace().nth(1)
            && row.split_whitespace().next() == Some("disk")
        {
            return Some(name.to_string());
        }
    }
    // a remastered ISO can mount the boot medium elsewhere; the mount source still names it
    let src = out("findmnt", &["-no", "SOURCE", "/run/archiso/bootmnt"])?;
    out("lsblk", &["-no", "PKNAME", src.trim()]).and_then(|s| first_word(&s))
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
pub fn clear_busy(disk: &str) {
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

/// Every unallocated MiB range on the disk, including gaps between existing partitions (a
/// Windows shrink leaves the free space before its recovery partition). One MiB is kept clear
/// before a following partition, because the plan passes `end + 1` to parted.
pub fn free_gaps(table: &str, total_mib: u64) -> Vec<(u64, u64)> {
    let usable_end = total_mib.saturating_sub(2);
    if usable_end < 2 {
        return Vec::new();
    }
    let mut used = table
        .lines()
        .filter_map(|line| {
            let fields = line.split(':').collect::<Vec<_>>();
            fields.first()?.parse::<u32>().ok()?;
            let start = fields.get(1)?.trim_end_matches("MiB").parse::<f64>().ok()? as u64;
            let end = fields.get(2)?.trim_end_matches("MiB").parse::<f64>().ok()? as u64;
            (start <= end).then_some((start, end))
        })
        .collect::<Vec<_>>();
    used.sort_unstable();

    let mut gaps = Vec::new();
    let mut cursor = 1u64;
    for (start, end) in used {
        let gap_end = start.saturating_sub(2);
        if gap_end > cursor {
            gaps.push((cursor, gap_end));
        }
        cursor = cursor.max(end.saturating_add(1));
    }
    if usable_end > cursor {
        gaps.push((cursor, usable_end));
    }
    gaps
}

/// The largest unallocated range, in MiB. Dualboot installs into this gap, not only into space
/// after the last partition.
pub fn largest_gap(table: &str, total_mib: u64) -> Option<(u64, u64)> {
    free_gaps(table, total_mib)
        .into_iter()
        .max_by_key(|(start, end)| end - start)
}

/// What step 2 does with a disk that already has partitions.
#[derive(Debug, PartialEq)]
pub enum EspChoice {
    /// Enough free space and a FAT32 ESP: reuse both.
    Reuse(u64),
    /// Enough free space, but the ESP needs a fresh FAT32 filesystem.
    Format(u64),
    /// Largest unallocated range, below the 16 GiB dualboot minimum.
    TooSmall(u64),
    /// No EFI System partition to reuse.
    NoEsp,
}

/// Pure keep/wipe decision; `table` is `parted -sm` output for `total_mib`.
pub fn keep_choice(table: &str, total_mib: u64, esp: Option<&str>, fstype: &str) -> EspChoice {
    let free = largest_gap(table, total_mib)
        .map_or(0, |(start, end)| end.saturating_sub(start).saturating_add(1));
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
            let fstype = rows(&listing)
                .iter()
                .find(|row| row.kind == "disk")
                .map(|row| row.fstype.clone())
                .unwrap_or_default();
            let in_use = rows(&listing).iter().any(|row| !row.mounts.is_empty());
            let state = if parts == 0 {
                if fstype.is_empty() {
                    "empty".to_string()
                } else {
                    // no partition table, but the whole disk holds a filesystem or LUKS
                    format!("whole-disk {}", fstype)
                }
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

/// Start of the largest unallocated range, in MiB.
pub fn free_start(disk: &str) -> u64 {
    let table = out("parted", &["-sm", disk, "unit", "MiB", "print"])
        .unwrap_or_else(|| die("cannot read the partition table"));
    largest_gap(&table, size_mib(disk))
        .map(|(start, _)| start)
        .unwrap_or_else(|| die("no unallocated space to install into"))
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
        EspChoice, dev_name, disk_names, free_gaps, keep_choice, largest_gap, partition_count,
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
    fn device_names_follow_the_disk_kind() {
        assert_eq!(dev_name("/dev/sda", 1), "/dev/sda1");
        assert_eq!(dev_name("/dev/nvme0n1", 1), "/dev/nvme0n1p1");
    }

    #[test]
    fn install_disks_skip_loop_and_rom_devices() {
        let rows = "loop loop0\nrom sr0\ndisk sda\ndisk nvme0n1\n";
        assert_eq!(disk_names(rows), ["sda", "nvme0n1"]);
    }

    #[test]
    fn keep_choice_reuses_only_a_fat_esp_with_enough_room() {
        let table = "BYT;\n/dev/sda:204800MiB:scsi:512:512:gpt:;\n1:1MiB:1023MiB:1022MiB:ext4:old-root:;\n";
        let free = 204800 - 1023 - 2;
        assert_eq!(keep_choice(table, 204800, Some("/dev/sda1"), "vfat"), EspChoice::Reuse(free));
        assert_eq!(keep_choice(table, 204800, Some("/dev/sda1"), "ext4"), EspChoice::Format(free));
        assert_eq!(keep_choice(table, 204800, None, ""), EspChoice::NoEsp);

        let big = "BYT;\n/dev/sda:204800MiB:scsi:512:512:gpt:;\n1:1MiB:190000MiB:189999MiB:ext4:old-root:;\n";
        assert_eq!(keep_choice(big, 204800, Some("/dev/sda1"), "vfat"), EspChoice::TooSmall(14798));
    }

    #[test]
    fn shrink_gap_between_windows_partitions_counts_as_free_space() {
        // Windows shrinks C:, leaving unallocated space before the recovery partition at the end
        let table = "BYT;\n/dev/nvme0n1:1000000MiB:scsi:512:512:gpt:;\n1:1MiB:260MiB:260MiB:fat32:EFI:boot,esp;\n2:261MiB:500000MiB:499740MiB:ntfs:Windows:msftdata;\n3:999749MiB:999998MiB:250MiB:ntfs:Recovery:;\n";
        assert_eq!(largest_gap(table, 1000000), Some((500001, 999747)));
        assert_eq!(free_gaps(table, 1000000), [(500001, 999747)]);
    }

    #[test]
    fn no_gap_when_the_last_partition_reaches_the_backup_gpt() {
        let table = "BYT;\n/dev/sda:204800MiB:scsi:512:512:gpt:;\n1:1MiB:204798MiB:204798MiB:ext4:full:;\n";
        assert!(free_gaps(table, 204800).is_empty());
        assert_eq!(largest_gap(table, 204800), None);
    }
}
