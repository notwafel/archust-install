//! Pre-install data destruction: extra disks and optionally the install disk.

use crate::config::{Config, ExtraDisk, ExtraFs};
use crate::disk;
use crate::steps::luks;
use crate::sys::{exists, out, run as sh};
use crate::ui::{die, say, warn};

const MAPPER: &str = "data";

pub fn run(cfg: &Config) {
    for extra in &cfg.extras {
        clean(extra, &cfg.extra_luks_pass);
    }
    if cfg.zero_install {
        disk::clear_busy(&cfg.disk);
        zero(&cfg.disk);
    }
}

fn clean(extra: &ExtraDisk, pass: &str) {
    require_tool(extra.fs);
    say(&format!("Cleaning {}", extra.dev));
    // parted and dd must not run on a mounted disk or a mapper held open by a failed run
    disk::clear_busy(&extra.dev);
    if extra.zero {
        zero(&extra.dev);
    }
    if extra.fs == ExtraFs::None {
        return;
    }
    let part = partition(&extra.dev);
    // stale exFAT/FAT labels survive mkfs on the same device node; wipe every signature first
    sh("wipefs", &["-a", &part]);
    let target = if extra.luks {
        // one shared mapper name; a failed earlier run can leave it open
        if exists(&format!("/dev/mapper/{}", MAPPER)) {
            die(&format!("close /dev/mapper/{} from the earlier run: cryptsetup close {}", MAPPER, MAPPER));
        }
        luks::encrypt(&part, pass, MAPPER)
    } else {
        part
    };
    mkfs(extra.fs, &target);
    if extra.luks {
        sh("cryptsetup", &["close", MAPPER]);
    }
}

/// The live ISO has to provide the formatter before anything is erased.
fn require_tool(fs: ExtraFs) {
    let program = match fs {
        ExtraFs::None => return,
        ExtraFs::Ext4 => "mkfs.ext4",
        ExtraFs::Btrfs => "mkfs.btrfs",
        ExtraFs::Xfs => "mkfs.xfs",
        ExtraFs::Exfat => "mkfs.exfat",
        ExtraFs::Fat32 => "mkfs.fat",
    };
    if !exists(&format!("/usr/bin/{}", program)) {
        die(&format!("/usr/bin/{} is missing from this ISO - use a current Arch ISO", program));
    }
}

/// One GPT partition over the whole disk, after the old table is gone.
fn partition(disk: &str) -> String {
    sh("parted", &["-s", disk, "mklabel", "gpt"]);
    sh("parted", &["-s", disk, "mkpart", "data", "1MiB", "100%"]);
    sh("partprobe", &[disk]);
    sh("udevadm", &["settle"]);
    let part = disk::dev_name(disk, 1);
    if !exists(&part) {
        die(&format!("{} did not appear after mkpart", part));
    }
    part
}

fn zero(dev: &str) {
    warn(&format!("overwriting {} with zeros, this can take hours", dev));
    // shortcut: logical sectors only; SSD wear levelling can keep old NAND cells.
    // Use blkdiscard or the drive's secure-erase command when physical erasure is required.
    // A bare dd to the device only ends by failing with "no space left on device", which the runner
    // reports as a failed step; `count` stops it at the end of the disk instead.
    let count = format!("count={}", zero_blocks(dev));
    sh("dd", &["if=/dev/zero", &format!("of={}", dev), "bs=1M", "status=progress", "conv=fsync", &count]);
}

/// Whole 1 MiB blocks on the device; a final partial block is left alone, because writing past the
/// end is exactly what dd reports as "no space left on device".
fn zero_blocks(dev: &str) -> u64 {
    let bytes = match out("blockdev", &["--getsize64", dev]).and_then(|s| s.trim().parse::<u64>().ok()) {
        Some(bytes) => bytes,
        None => return disk::size_mib(dev),
    };
    bytes / 1048576
}

fn mkfs(fs: ExtraFs, dev: &str) {
    match fs {
        ExtraFs::None => {}
        ExtraFs::Ext4 => sh("mkfs.ext4", &["-F", "-L", "data", dev]),
        ExtraFs::Btrfs => sh("mkfs.btrfs", &["-f", "-L", "data", dev]),
        ExtraFs::Xfs => sh("mkfs.xfs", &["-f", "-L", "data", dev]),
        ExtraFs::Exfat => sh("mkfs.exfat", &["-n", "data", dev]),
        ExtraFs::Fat32 => sh("mkfs.fat", &["-F32", "-n", "DATA", dev]),
    }
}
