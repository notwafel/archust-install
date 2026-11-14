//! Question sequence, archinstall-style menus over arrow keys.

use crate::config::Config;
use crate::detect;
use crate::disk;
use crate::fs::{self, Fs};
use crate::layout::{plan, Request};
use crate::term::{ask, ask_name, ask_num, ask_num_min, ask_yes, pick, pick_many};
use crate::ui::{die, note, say, warn};

const KERNELS: [&str; 3] = ["linux", "linux-lts", "linux-zen"];
const KEYMAPS: [&str; 12] = ["us", "uk", "de", "fr", "es", "it", "pl", "pt", "nl", "se", "no", "dk"];
const LOCALES: [&str; 12] = ["en_US", "en_GB", "de_DE", "fr_FR", "es_ES", "it_IT", "pl_PL", "pt_PT", "nl_NL", "sv_SE", "nb_NO", "da_DK"];
const SHELLS: [&str; 3] = ["/bin/bash", "/bin/zsh", "/usr/bin/fish"];
const ZONES: [&str; 15] = [
    "UTC", "Europe/Warsaw", "Europe/London", "Europe/Berlin", "Europe/Paris",
    "Europe/Madrid", "Europe/Rome", "Europe/Lisbon", "Europe/Amsterdam",
    "America/New_York", "America/Chicago", "America/Denver", "America/Los_Angeles",
    "Asia/Tokyo", "Australia/Sydney",
];

const BASE: [&str; 13] = [
    "base", "base-devel", "linux-headers", "linux-firmware", "dkms", "locales",
    "grub", "efibootmgr", "dosfstools", "mtools", "networkmanager", "sudo", "bash-completion",
];

/// Optional groups, pre-ticked. Unchecked ones are not installed.
const EXTRAS: [(&str, &str); 9] = [
    ("nano", "nano"),
    ("curl", "curl"),
    ("git", "git"),
    ("flatpak", "flatpak"),
    ("pipewire", "pipewire pipewire-pulse pipewire-alsa wireplumber alsa-utils"),
    ("man-db", "man-db"),
    ("reflector", "reflector"),
    ("graphics", "mesa vulkan-icd-loader libva-utils"),
    // extra firmware: laptops and new Wi-Fi cards need these, plain linux-firmware is not enough
    ("firmware", "linux-firmware-intel linux-firmware-realtek linux-firmware-nvidia"),
];

const MULTILIB: [&str; 5] = ["lib32-mesa", "lib32-vulkan-icd-loader", "lib32-libva", "lib32-alsa-plugins", "lib32-gnutls"];

const WAYLAND: [&str; 12] = [
    "wayland", "wayland-protocols", "xorg-xwayland", "libxkbcommon", "xkeyboard-config",
    "libinput", "xf86-input-libinput", "seatd", "mesa-utils", "vulkan-tools",
    "xdg-desktop-portal", "xdg-utils",
];

/// Re-asks the partition questions when `plan()` rejects the sizes.
fn ask_layout(
    total_mib: u64,
    dual: bool,
    free_start: u64,
    boot_mib: u64,
) -> (u64, Vec<crate::layout::Part>) {
    loop {
        let swap_gib = ask_num("Swap, also for hibernation (GiB, 0 = none)", 4);
        let root_gib = ask_num("Root (GiB, 0 = rest of disk)", 0);
        let parts = plan(Request {
            total_mib,
            boot_mib,
            swap_mib: swap_gib * 1024,
            root_mib: root_gib * 1024,
            dualboot: dual,
            free_start,
        });
        match parts {
            Some(parts) => return (swap_gib, parts),
            None => warn("does not fit on this disk, try smaller sizes"),
        }
    }
}

pub fn ask_everything() -> Config {
    let disk = ask_disk();
    let total_mib = disk::size_mib(&disk);
    let has_parts = disk::has_partitions(&disk);

    let dual = has_parts && ask_yes("Dualboot (keep existing partitions, new ones at the end)?", false);
    let (esp_part, free_start, boot_mib) = if dual {
        let esp = disk::existing_esp(&disk)
            .unwrap_or_else(|| die(&format!("dualboot: no EFI partition on {}", disk)));
        let free = disk::free_start(&disk);
        if free >= total_mib {
            die("no free space after the last partition");
        }
        say(&format!("reusing {} as /boot", esp));
        (Some(esp), free, 0)
    } else {
        warn(&format!("This wipes ALL of {} - irreversible.", disk));
        if !ask_yes("Confirm ERASE", false) {
            die("aborted");
        }
        (None, 1, ask_num_min("EFI partition (MiB)", 1024, 300))
    };

    let (swap_gib, parts) = ask_layout(total_mib, dual, free_start, boot_mib);
    let fs = pick("Filesystem", &fs::CHOICES, 0);
    let luks = ask_yes("LUKS encryption?", false);
    let hibernate = swap_gib >= 1
        && detect::memory_gib() <= swap_gib
        && ask_yes("Hibernate to swap (resume from disk)?", true);
    show_plan(&disk, total_mib, dual, luks, fs, esp_part.as_deref(), &parts);

    let kernel = pick("Kernel", &KERNELS, 0);
    let gpu = detect::ask_gpu();
    let multilib = ask_yes("Enable multilib (32-bit libs, Steam/Wine/games)?", true);
    let wayland = ask_yes("Wayland + XWayland stack now (ready for a compositor)?", true);

    let groups = EXTRAS.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    let checked = vec![true; groups.len()];
    let mut extras: Vec<&str> = Vec::new();
    for name in pick_many("Extra packages", &groups, &checked) {
        if let Some((_, pkgs)) = EXTRAS.iter().find(|(n, _)| *n == name) {
            extras.extend(pkgs.split(' '));
        }
    }

    let hostname = ask_name("Hostname");
    let user = ask_name("Username");
    let tz = pick_zone();
    let keymap = pick("Keyboard layout", &KEYMAPS, 0).to_string();
    let locale = pick("Locale", &LOCALES, 0).to_string();
    let shell = pick("Login shell", &SHELLS, 0).to_string();
    let swap = parts
        .iter()
        .find(|p| p.role == crate::layout::Role::Swap)
        .map(|p| (disk::dev_name(&disk, partition_index(&disk, &parts, p)), hibernate));
    let luks_pass = if luks { crate::term::ask_pass("LUKS passphrase") } else { String::new() };
    let user_pass = crate::term::ask_pass("User password");
    let root_pass = crate::term::ask_pass("Root password");

    let mut packages = BASE.to_vec();
    packages.extend(extras);
    packages.push(kernel);
    packages.extend(gpu.packages());
    if let Some(ucode) = detect::cpu_ucode() {
        packages.push(ucode);
    }
    if wayland {
        packages.extend(WAYLAND);
    }
    // shortcut: Steam/Proton also want lib32-vulkan-{radeon,intel,nvidia-utils};
    // add them when a real game setup is requested, not for "ready for a DE"
    if multilib {
        packages.extend(MULTILIB);
        if gpu.nvidia() {
            packages.push("lib32-nvidia-utils");
        }
    }
    if let Some(pkg) = shell_package(&shell) {
        packages.push(pkg);
    }
    if luks {
        packages.push("cryptsetup");
    }
    if dual {
        packages.push("os-prober");
    }
    packages.sort_unstable();
    packages.dedup();

    Config {
        disk,
        dual,
        esp_part,
        parts,
        luks,
        multilib,
        fs,
        gpu,
        hostname,
        tz,
        keymap,
        locale,
        user,
        shell,
        luks_pass,
        user_pass,
        root_pass,
        packages,
        swap,
    }
}

/// bash is already in `base`; the others have to be installed or the login fails.
fn shell_package(shell: &str) -> Option<&'static str> {
    match shell {
        "/bin/zsh" => Some("zsh"),
        "/usr/bin/fish" => Some("fish"),
        _ => None,
    }
}

/// 1-based partition number: 1 is the ESP on a wiped disk, otherwise the last existing + offset.
fn partition_index(disk: &str, parts: &[crate::layout::Part], part: &crate::layout::Part) -> u32 {
    let position = parts.iter().position(|p| p.start == part.start).unwrap_or(0) + 1;
    let base = if disk::has_partitions(disk) { disk::last_number(disk) } else { 0 };
    base + position as u32
}

fn pick_zone() -> String {    let answer = pick("Timezone", &ZONES, 0).to_string();
    let path = format!("/usr/share/zoneinfo/{}", answer);
    if std::path::Path::new(&path).exists() {
        answer
    } else {
        warn(&format!("{} missing on this ISO, using UTC", answer));
        "UTC".to_string()
    }
}

fn ask_disk() -> String {
    say("Disks");
    note(&disk::list());
    loop {
        let d = ask("Install disk (e.g. /dev/nvme0n1)", "");
        if disk::is_whole_disk(&d) {
            note(&disk::describe(&d));
            return d;
        }
        warn(&format!("{} is not a whole disk", d));
    }
}

fn show_plan(
    disk: &str,
    total_mib: u64,
    dual: bool,
    luks: bool,
    fs: Fs,
    esp: Option<&str>,
    parts: &[crate::layout::Part],
) {
    say(&format!("Plan: {} ({} GiB), dualboot={}, LUKS={}, fs={}", disk, total_mib / 1024, dual, luks, fs));
    if let Some(e) = esp {
        note(&format!("  {:<16} {:<10} ESP /boot (existing)", e, "-"));
    }
    for p in parts {
        let extra = if p.role == crate::layout::Role::Root && luks { " (LUKS2)" } else { "" };
        note(&format!("  {:<16} {:<10} {:?}{}", "new partition", format!("{}MiB", p.size()), p.role, extra));
    }
    if fs.has_subvolumes() {
        note(&format!("  btrfs subvolumes: {}", fs::SUBVOLS.join(" ")));
    } else {
        note("  single root filesystem, no subvolumes");
    }
}
