//! Question sequence, archinstall-style menus over arrow keys, one numbered section per step.

use crate::config::Config;
use crate::detect;
use crate::disk;
use crate::fs::{self, Fs};
use crate::layout::{self, plan, Request};
use crate::term::{ask, ask_name, ask_num, ask_yes, pick, pick_disk, pick_fuzzy, pick_many};
use std::path::Path;
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

const BASE: [&str; 12] = [
    "base", "base-devel", "linux-headers", "linux-firmware", "dkms", "locales",
    "grub", "efibootmgr", "dosfstools", "mtools", "networkmanager", "sudo",
];

/// Optional groups, pre-ticked. Unchecked ones are not installed. Firmware is not here:
/// it depends on the selected GPU, see `extras_for`.
const EXTRAS: [(&str, &str); 8] = [
    ("Nano editor", "nano"),
    ("curl", "curl"),
    ("git", "git"),
    ("Flatpak", "flatpak"),
    ("PipeWire audio", "pipewire pipewire-pulse pipewire-alsa wireplumber alsa-utils"),
    ("Manual pages", "man-db"),
    ("Reflector mirrors", "reflector"),
    ("Graphics: Mesa, Vulkan, VA tools", "mesa vulkan-icd-loader libva-utils"),
];

/// `linux-firmware-intel` and `-realtek` cover Wi-Fi, Ethernet and Bluetooth chips, not only
/// GPUs, so only the NVIDIA blob and the VM case follow the GPU answer.
fn extras_for(gpu: detect::Gpu) -> Vec<(&'static str, &'static str)> {
    let mut groups = EXTRAS.to_vec();
    match gpu {
        detect::Gpu::Virt => {}
        detect::Gpu::Nvidia | detect::Gpu::NvidiaOpen | detect::Gpu::Nouveau => groups.push((
            "Firmware: Intel, Realtek, NVIDIA",
            "linux-firmware-intel linux-firmware-realtek linux-firmware-nvidia",
        )),
        detect::Gpu::Intel | detect::Gpu::Amd => groups.push((
            "Firmware: Intel, Realtek",
            "linux-firmware-intel linux-firmware-realtek",
        )),
    }
    groups
}

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
    let cpu = detect::ask_cpu();
    let gpu = detect::ask_gpu();

    step(6, STEPS, "Packages");
    let multilib = ask_yes("Enable multilib (32-bit libs, Steam/Wine/games)?", true);
    let wayland = ask_yes("Wayland + XWayland stack now (ready for a compositor)?", true);
    let extras_groups = extras_for(gpu);
    let groups = extras_groups.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    let checked = vec![true; groups.len()];
    let mut extras: Vec<&str> = Vec::new();
    for name in pick_many("Extra packages", &groups, &checked) {
        if let Some((_, pkgs)) = extras_groups.iter().find(|(n, _)| *n == name) {
            extras.extend(pkgs.split(' '));
        }
    }

    step(7, STEPS, "System");
    let hostname = ask_name("Hostname");
    let user = ask_name("Username");
    let (tz, keymap, locale, shell) = ask_system_options();

    step(8, STEPS, "Passwords");
    let luks_pass = if luks { crate::term::ask_pass("LUKS passphrase") } else { String::new() };
    let user_pass = crate::term::ask_pass("User password");
    let root_pass = crate::term::ask_pass("Root password");

    let mut packages = BASE.to_vec();
    packages.extend(extras);
    packages.push(kernel);
    packages.extend(gpu.packages());
    // hybrid laptop: without this the integrated GPU gets no Vulkan/VA driver at all
    if matches!(gpu, detect::Gpu::Nvidia | detect::Gpu::NvidiaOpen | detect::Gpu::Nouveau)
        && let Some(igpu) = detect::companion()
    {
        packages.extend(igpu.packages());
    }
    if let Some(ucode) = cpu.ucode() {
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
    if shell == "/bin/bash" {
        packages.push("bash-completion");
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
                    let kind = disk::fs_type(&esp);
                    if kind != "vfat" {
                        die(&format!(
                            "{} is {} - the installer needs a FAT32 ESP, format it or pick another disk",
                            esp,
                            if kind.is_empty() { "unformatted" } else { &kind }
                        ));
                    }
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

    note("Recommended: swap 4 GiB, root 0 (all remaining), EFI 1024 MiB.");
    loop {
        let swap_gib = ask_num("Swap GiB, also for hibernation (0 = none, 4 = recommended)", 4);
        let room = space.saturating_sub(boot_mib).saturating_sub(swap_gib * 1024);
        let root_gib = ask_num(&format!("Root GiB (0 = all remaining, about {} MiB)", room), 0);
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
        let mib = ask_num("EFI partition MiB (1024 recommended, 300 minimum)", 1024);
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

fn ask_system_options() -> (String, String, String, String) {
    let zones = zone_choices();
    let keymaps = keymap_choices();
    let locales = locale_choices();
    let shells = SHELLS.iter().map(ToString::to_string).collect::<Vec<_>>();
    let mut tz = "UTC".to_string();
    let mut keymap = "us".to_string();
    let mut locale = "en_US".to_string();
    let shell = "/bin/bash".to_string();
    let mut field = 0;

    loop {
        match field {
            0 => if let Some(value) = pick_fuzzy("Timezone (type to search, Esc = back)", &zones, index_of(&zones, &tz)) {
                tz = value;
                field = 1;
            },
            1 => match pick_fuzzy("Keyboard layout (type to search, Esc = back)", &keymaps, index_of(&keymaps, &keymap)) {
                Some(value) => { keymap = value; field = 2; }
                None => field = 0,
            },
            2 => match pick_fuzzy("Locale (type to search, Esc = back)", &locales, index_of(&locales, &locale)) {
                Some(value) => { locale = value; field = 3; }
                None => field = 1,
            },
            _ => match pick_fuzzy("Login shell (type to search, Esc = back)", &shells, index_of(&shells, &shell)) {
                Some(value) => return (tz, keymap, locale, value),
                None => field = 2,
            },
        }
    }
}

fn index_of(items: &[String], value: &str) -> usize {
    items.iter().position(|item| item == value).unwrap_or(0)
}

fn zone_choices() -> Vec<String> {
    let mut zones = files_below(Path::new("/usr/share/zoneinfo"));
    zones.retain(|zone| {
        !zone.starts_with("posix/")
            && !zone.starts_with("right/")
            && !["iso3166.tab", "leapseconds", "localtime", "posixrules", "tzdata.zi", "zone.tab", "zone1970.tab"].contains(&zone.as_str())
    });
    choices_or(zones, &ZONES)
}

fn keymap_choices() -> Vec<String> {
    let mut keymaps = files_below(Path::new("/usr/share/kbd/keymaps"));
    keymaps = keymaps.into_iter().filter_map(|map| {
        map.strip_suffix(".map.gz").or_else(|| map.strip_suffix(".map")).map(str::to_string)
    }).collect();
    choices_or(keymaps, &KEYMAPS)
}

fn locale_choices() -> Vec<String> {
    let locales = std::fs::read_to_string("/etc/locale.gen")
        .map(|body| parse_utf8_locales(&body))
        .unwrap_or_default();
    choices_or(locales, &LOCALES)
}

fn parse_utf8_locales(body: &str) -> Vec<String> {
    body.lines().filter_map(|line| {
        let mut fields = line.trim_start().trim_start_matches('#').split_whitespace();
        let locale = fields.next()?;
        (fields.next() == Some("UTF-8"))
            .then(|| locale.strip_suffix(".UTF-8").map(str::to_string))
            .flatten()
    }).collect()
}

fn files_below(root: &Path) -> Vec<String> {
    fn visit(root: &Path, dir: &Path, files: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, files);
            } else if path.is_file() && let Ok(relative) = path.strip_prefix(root) {
                files.push(relative.to_string_lossy().into_owned());
            }
        }
    }

    let mut files = Vec::new();
    visit(root, root, &mut files);
    files
}

fn choices_or(mut choices: Vec<String>, fallback: &[&str]) -> Vec<String> {
    choices.sort_unstable();
    choices.dedup();
    if choices.is_empty() {
        fallback.iter().map(ToString::to_string).collect()
    } else {
        choices
    }
}

fn ask_disk() -> String {
    step(1, STEPS, "Disk");
    let found = disk::disks();
    if found.is_empty() {
        die("no disks found besides the live ISO's own");
    }
    note("Enter picks the highlighted disk. The live ISO's own disk is hidden.");
    note("Size, model and partition count come from lsblk; \"in use\" means mounted or swap.");
    let disk = format!("/dev/{}", found[pick_disk(&disk::menu(&found))]);
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

#[cfg(test)]
mod tests {
    use super::{extras_for, parse_utf8_locales};
    use crate::detect::Gpu;

    #[test]
    fn lists_only_utf8_locale_names() {
        let locales = parse_utf8_locales("#en_US.UTF-8 UTF-8\npl_PL.UTF-8 UTF-8\n#de_DE ISO-8859-1\n");
        assert_eq!(locales, ["en_US", "pl_PL"]);
    }

    #[test]
    fn firmware_group_follows_the_gpu() {
        let groups = |gpu| extras_for(gpu).iter().map(|(name, _)| *name).collect::<Vec<_>>();
        assert!(!groups(Gpu::Virt).iter().any(|name| name.starts_with("Firmware")));
        assert!(groups(Gpu::Amd).contains(&"Firmware: Intel, Realtek"));
        assert!(groups(Gpu::Nvidia).contains(&"Firmware: Intel, Realtek, NVIDIA"));
    }
}
