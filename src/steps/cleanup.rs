//! Pre-install data destruction: extra disks and optionally the install disk.

use crate::config::{Config, ExtraDisk, ExtraFs};
use crate::disk;
use crate::steps::luks;
use crate::sys::{exists, ok, out, run as sh};
use crate::ui::{die, note, say, warn};

const MAPPER: &str = "data";

pub fn run(cfg: &Config) {
    for extra in &cfg.extras {
        clean(extra, &cfg.extra_luks_pass);
    }
    if cfg.zero_install {
        disk::clear_busy(&cfg.disk);
        erase(&cfg.disk);
    }
}

fn clean(extra: &ExtraDisk, pass: &str) {
    require_tool(extra.fs);
    say(&format!("Cleaning {}", extra.dev));
    // parted and dd must not run on a mounted disk or a mapper held open by a failed run
    disk::clear_busy(&extra.dev);
    if extra.zero {
        erase(&extra.dev);
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

/// NVMe goes through sanitize. Otherwise a drive that answered `hdparm -I` gets its ATA secure
/// erase: that also covers USB bridges that translate ATA commands, which is why the transport
/// alone is not enough. Pure, so the choice is testable without a disk.
pub fn erase_plan(transport: &str, ata_security: bool) -> Erase {
    if transport.trim() == "nvme" {
        Erase::Nvme
    } else if ata_security {
        Erase::Ata
    } else {
        Erase::Overwrite
    }
}

/// What the drive does with its own media. An overwrite from the host never reaches the cells the
/// controller keeps in reserve, so it is only the fallback.
#[derive(Debug, PartialEq)]
pub enum Erase {
    Nvme,
    Ata,
    Overwrite,
}

/// Erases every sector the controller owns, including the cells the drive keeps in reserve.
/// An overwrite from the host never reaches those, so it is only the fallback.
fn erase(dev: &str) {
    let transport = out("lsblk", &["-dno", "TRAN", dev]).unwrap_or_default();
    // `hdparm -I` also answers for USB-SATA bridges, which is why it is probed instead of TRAN
    let health = ok("hdparm", &["-I", dev]);
    match erase_plan(&transport, health) {
        Erase::Nvme => nvme(dev),
        Erase::Ata => ata(dev),
        Erase::Overwrite => overwrite(dev),
    }
}

/// `nvme sanitize` is the only command that covers the whole NVM subsystem. Sanitize capability
/// bits: 0x1 block erase, 0x2 crypto erase, 0x4 overwrite.
fn nvme(dev: &str) {
    let ctrl = controller(dev);
    let cap = sanicap(&ctrl);
    if cap == 0 {
        // a format still erases all user data "regardless of location (within deallocated LBAs,
        // cache, etc)" per man nvme-format(1); a cryptographic erase covers the media at once
        warn(&format!("{} reports no sanitize support, formatting instead", dev));
        if !ok("nvme", &["format", dev, "-s", "2"]) {
            warn("cryptographic erase unsupported, erasing user data instead");
            sh("nvme", &["format", dev, "-s", "1"]);
        }
        say(&format!("{} formatted, all user data erased", dev));
        return;
    }
    let mut args = vec!["sanitize", ctrl.as_str()];
    if cap & 0x2 != 0 {
        args.extend(["-a", "4"]); // crypto erase: drops the key, covers every cell at once
    } else if cap & 0x1 != 0 {
        args.extend(["-a", "2"]); // block erase: the controller rewrites the whole media
    } else {
        args.extend(["-a", "3", "-n", "1"]); // only overwrite is advertised; one pass is enough
    }
    args.push("--preq"); // refuse a success that does not purge user data per IEEE 2883
    say(&format!("{} sanitize, this takes minutes and cannot be interrupted", dev));
    sh("nvme", &args);
    wait_sanitize(&ctrl);
    say(&format!("{} sanitized, sanitize-log reports success", dev));
}

/// The character device takes the sanitize command; the namespace path is accepted by the tool
/// but the controller is the documented target.
fn controller(dev: &str) -> String {
    // /dev/nvme0n1 -> /dev/nvme0, /dev/nvme0 -> /dev/nvme0
    match dev.rsplit_once('n') {
        Some((head, tail)) if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) => head.to_string(),
        _ => dev.to_string(),
    }
}

/// Bits 0-2 of SANICAP. A missing field means an old drive with no sanitize command at all.
fn sanicap(ctrl: &str) -> u8 {
    sanicap_from(&out("nvme", &["id-ctrl", ctrl]).unwrap_or_default())
}

fn sanicap_from(id_ctrl: &str) -> u8 {
    id_ctrl
        .lines()
        .find_map(|line| line.trim().strip_prefix("sanicap"))
        .and_then(|v| v.trim().strip_prefix(':'))
        .and_then(|v| v.split_whitespace().next())
        .and_then(|v| v.trim_end_matches(',').strip_prefix("0x"))
        .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        .unwrap_or(0)
}

/// Sanitize runs asynchronously; the log page reports 0x2 while in progress and 0x1 on success.
/// A failed operation leaves the drive in a failure state where it accepts nothing else, so it
/// stops the install instead of letting partitioning fail with an unrelated message.
fn wait_sanitize(ctrl: &str) {
    say("waiting for the sanitize to finish");
    const LIMIT: usize = 720;
    for attempt in 0..=LIMIT {
        let log = out("nvme", &["log", "sanitize", ctrl]).unwrap_or_default();
        match sanitize_state(&log) {
            Some(1) => return,
            Some(3) => die(&format!("{} reports a failed sanitize", ctrl)),
            _ => {}
        }
        if attempt == LIMIT {
            die(&format!("{} sanitize did not finish within an hour", ctrl));
        }
        std::process::Command::new("sleep").arg("5").status().ok();
    }
}

/// The sanitize-log status field: 0 never sanitized, 1 success, 2 in progress, 3 failed. The log
/// is either a table row or `<field>: 0x0001`, so the status line's first code is taken; `None`
/// means the output was not recognized.
fn sanitize_state(log: &str) -> Option<u32> {
    let line = log.lines().find(|l| l.to_lowercase().contains("status"))?;
    let value = line
        .split([':', '=', '|', '\t', ' '])
        .map(str::trim)
        .find(|w| w.starts_with("0x") || (!w.is_empty() && w.chars().all(|c| c.is_ascii_digit())))?
        .trim_end_matches(',');
    match value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => value.parse().ok(),
    }
}

/// The drive's own ATA SECURE ERASE rewrites the whole media, including sectors the host cannot
/// see. The temporary password is cleared by the firmware when the erase completes.
fn ata(dev: &str) {
    let id = out("hdparm", &["-I", dev]).unwrap_or_default();
    // `hdparm -I` prints an empty security section when the bridge or the drive rejects the query
    let security = id.split("Security:").nth(1).unwrap_or("");
    let line = |wanted: &str| security.lines().any(|l| l.trim() == wanted);
    if !line("supported") {
        warn(&format!("{} does not report the ATA security feature set", dev));
        overwrite(dev);
        return;
    }
    // a frozen drive accepts no security command until the next power-on; no command can unfreeze
    // it from the host, and the erase cannot run
    if line("frozen") {
        die(&format!(
            "{} is security-frozen: power the machine off and erase it again before selling it",
            dev
        ));
    }
    // enhanced erase rewrites the media itself; plain erase may only mark the sectors gone
    let enhanced = security.lines().any(|l| l.trim() == "supported: enhanced erase");
    let pass = "archust-erase";
    say(&format!("{} secure erase, the drive takes minutes to hours", dev));
    if !ok("hdparm", &["--user-master", "u", "--security-set-pass", pass, dev]) {
        die(&format!("{} refused the erase password, so it cannot be secure-erased", dev));
    }
    let flag = if enhanced { "--security-erase-enhanced" } else { "--security-erase" };
    if !ok("hdparm", &["--user-master", "u", flag, pass, dev]) {
        // a failed erase leaves the drive locked behind that password: clear it before anything
        // else, or the disk is unusable and unsellable
        if !ok("hdparm", &["--user-master", "u", "--security-disable", pass, dev]) {
            die(&format!("{} kept the erase password; run hdparm --security-disable {} {}", dev, pass, dev));
        }
        die(&format!("{} did not complete the secure erase; its data is not verifiably gone", dev));
    }
    say(&format!("{} erased by its own controller", dev));
}

/// Last resort: no erase command exists on this device, so every logical sector the host can
/// address is written over. Cells the controller keeps in reserve are not covered - this is not
/// a guarantee for a disk that leaves your hands.
fn overwrite(dev: &str) {
    note(&format!("{} has no erase command, overwriting every sector", dev));
    warn(&format!("an overwrite is not a guarantee on flash media: use a drive-specific tool for {}", dev));
    zero(dev);
}

fn zero(dev: &str) {
    warn(&format!("overwriting {} with zeros, this can take hours", dev));
    // shortcut: logical sectors only; SSD wear levelling can keep old NAND cells.
    // Use the drive's erase command when physical erasure is required.
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
#[cfg(test)]
mod tests {
    use super::{Erase, controller, erase_plan, sanicap_from, sanitize_state};

    #[test]
    fn erase_uses_the_drive_command_when_it_has_one() {
        assert_eq!(erase_plan("nvme\n", false), Erase::Nvme);
        // a SATA disk whose ATA Security section could be read
        assert_eq!(erase_plan("sata", true), Erase::Ata);
        // flash media behind USB: no usable ATA command, and a host overwrite is all that is left
        assert_eq!(erase_plan("sata", false), Erase::Overwrite);
        // a USB bridge that answers ATA commands can still secure erase
        assert_eq!(erase_plan("usb", true), Erase::Ata);
        assert_eq!(erase_plan("", false), Erase::Overwrite);
    }

    #[test]
    fn sanitize_targets_the_controller_not_a_namespace() {
        assert_eq!(controller("/dev/nvme0n1"), "/dev/nvme0");
        assert_eq!(controller("/dev/nvme0n10"), "/dev/nvme0");
        assert_eq!(controller("/dev/nvme0"), "/dev/nvme0");
    }

    #[test]
    fn sanitize_capability_is_read_from_the_identify_block() {
        assert_eq!(sanicap_from("mnvme_0\nsanicap : 0x6\nsqes : 0x66\n"), 0x6);
        assert_eq!(sanicap_from("sanicap : 0x2\n"), 0x2);
        assert_eq!(sanicap_from("sanicap : 0x0\n"), 0);
        assert_eq!(sanicap_from("no such field\n"), 0);
    }

    #[test]
    fn sanitize_status_is_read_in_both_output_shapes() {
        assert_eq!(sanitize_state("Sanitize status: 0x0001\n"), Some(1));
        assert_eq!(sanitize_state("Sanitize status : 0x0002\n"), Some(2));
        assert_eq!(sanitize_state("san_status     : 3\n"), Some(3));
        assert_eq!(sanitize_state("==> no idea\n"), None);
        assert_eq!(sanitize_state(""), None);
    }
}
