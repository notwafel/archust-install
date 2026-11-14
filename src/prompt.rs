//! Question sequence, archinstall-style menus over arrow keys, one numbered section per step.

use crate::config::Config;
use crate::detect;
use crate::disk;
use crate::fs::{self, Fs};
use crate::layout::{self, plan, Request};
use crate::term::{ask, ask_name, ask_num, ask_yes, pick, pick_disk, pick_many};
use crate::ui::{die, item, note, say, step, warn};

const STEPS: usize = 8;

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

/// What step 2 decided: keep the disk (reuse ESP, start after the last partition) or wipe it.
struct Keep {
    dual: bool,
    esp_part: Option<String>,
    free_start: u64,
    boot_mib: u64,
}

/// Everything the layout questions produced, before the rest of the answers exist.
struct Layout {
    dual: bool,
    esp_part: Option<String>,
    parts: Vec<layout::Part>,
    hibernate: bool,
}

pub fn ask_everything() -> Config {
    let disk = ask_disk();
    let total_mib = disk::size_mib(&disk);
    let layout_answers = ask_layout(&disk, total_mib);

    step(4, STEPS, "Filesystem and encryption");
    let fs = pick("Filesystem", &fs::CHOICES, 0);
    let luks = ask_yes("LUKS encryption?", false);
    show_plan(&disk, total_mib, &layout_answers, fs, luks);

    step(5, STEPS, "Hardware");
    let kernel = pick("Kernel", &KERNELS, 0);
    let gpu = detect::ask_gpu();

    step(6, STEPS, "Packages");
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

    step(7, STEPS, "System");
    let hostname = ask_name("Hostname");
    let user = ask_name("Username");
    let tz = pick_zone();
    let keymap = pick("Keyboard layout", &KEYMAPS, 0).to_string();
    let locale = pick("Locale", &LOCALES, 0).to_string();
    let shell = pick("Login shell", &SHELLS, 0).to_string();

    step(8, STEPS, "Passwords");
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
    if layout_answers.dual {
        packages.push("os-prober");
    }
    packages.sort_unstable();
    packages.dedup();

    Config {
        disk,
        dual: layout_answers.dual,
        esp_part: layout_answers.esp_part,
        parts: layout_answers.parts,
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
        hibernate: layout_answers.hibernate,
    }
}

/// Steps 2-3: keep the disk or wipe it, then the sizes. Re-asks until the plan fits.
fn ask_layout(disk: &str, total_mib: u64) -> Layout {
    step(2, STEPS, "Partitioning");
    let Keep { dual, esp_part, free_start, boot_mib } = if disk::has_partitions(disk) {
        note(&disk::table(disk));
        if ask_yes("Keep the existing partitions and install into the free space?", true) {
            let free = disk::free_mib(disk, total_mib);
            match disk::existing_esp(disk) {
                Some(esp) if free >= 16384 => {
                    say(&format!("reusing {} as /boot", esp));
                    item("free space", &format!("{} MiB", free));
                    Keep { dual: true, esp_part: Some(esp), free_start: disk::free_start(disk), boot_mib: 0 }
                }
                Some(_) => {
                    warn(&format!("only {} MiB free after the last partition, 16384 MiB needed", free));
                    wipe(disk)
                }
                None => {
                    warn(&format!("{} has no EFI partition, a new one has to be created", disk));
                    wipe(disk)
                }
            }
        } else {
            wipe(disk)
        }
    } else {
        say(&format!("{} is empty", disk));
        wipe_esp()
    };

    step(3, STEPS, "Sizes");
    let space = layout::space_mib(&Request {
        total_mib, boot_mib, swap_mib: 0, root_mib: 0, dualboot: dual, free_start,
    });
    item("usable", &format!("{} MiB{}", space, if dual { " (free space)" } else { "" }));

    loop {
        let swap_gib = ask_num("Swap, also for hibernation (GiB, 0 = none)", 4);
        let root_gib = ask_num("Root (GiB, 0 = rest of the usable space)", 0);
        let parts = plan(Request {
            total_mib,
            boot_mib,
            swap_mib: swap_gib * 1024,
            root_mib: root_gib * 1024,
            dualboot: dual,
            free_start,
        });
        let Some(parts) = parts else {
            warn(&format!("does not fit in {} MiB of usable space, try smaller sizes", space));
            continue;
        };
        let hibernate = swap_gib >= 1
            && detect::memory_gib() <= swap_gib
            && ask_yes("Hibernate to swap (resume from disk)?", true);
        return Layout { dual, esp_part, parts, hibernate };
    }
}

/// Wipe branch: the disk name has to be typed back, then a fresh EFI partition.
fn wipe(disk: &str) -> Keep {
    warn(&format!("This wipes ALL of {} - irreversible.", disk));
    if ask(&format!("Type {} to confirm", disk)) != disk {
        die("aborted");
    }
    wipe_esp()
}

fn wipe_esp() -> Keep {
    loop {
        let mib = ask_num("EFI partition (MiB)", 1024);
        if mib >= 300 {
            return Keep { dual: false, esp_part: None, free_start: 1, boot_mib: mib };
        }
        warn("the EFI partition needs at least 300 MiB");
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

fn pick_zone() -> String {
    let answer = pick("Timezone", &ZONES, 0).to_string();
    let path = format!("/usr/share/zoneinfo/{}", answer);
    if std::path::Path::new(&path).exists() {
        answer
    } else {
        warn(&format!("{} missing on this ISO, using UTC", answer));
        "UTC".to_string()
    }
}

fn ask_disk() -> String {
    step(1, STEPS, "Disk");
    let found = disk::disks();
    if found.is_empty() {
        die("no disks found besides the live ISO's own");
    }
    note(&disk::disks_listing());
    let disk = format!("/dev/{}", found[pick_disk(&found)]);
    note(&disk::describe(&disk));
    disk
}

fn show_plan(disk: &str, total_mib: u64, layout: &Layout, fs: Fs, luks: bool) {
    say(&format!("Plan on {} ({} GiB)", disk, total_mib / 1024));
    item("keep existing", if layout.dual { "yes" } else { "no, full wipe" });
    item("encryption", if luks { "LUKS2 on root" } else { "none" });
    item("filesystem", &fs.to_string());
    if let Some(e) = &layout.esp_part {
        item("ESP /boot", &format!("{} (existing)", e));
    }
    for p in &layout.parts {
        let extra = if p.role == layout::Role::Root && luks { " (LUKS2)" } else { "" };
        item(
            &format!("{:?}", p.role),
            &format!("{} MiB {}{}", p.size(), if layout.dual { "at end of disk" } else { "new partition" }, extra),
        );
    }
    if fs.is_btrfs() {
        item("subvolumes", &fs::SUBVOLS.join(" "));
    } else {
        item("subvolumes", "none, single root filesystem");
    }
    crate::ui::rule();
}
