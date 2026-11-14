//! Step 5: locale, keymap, hostname, GRUB, initramfs, sysctl, users, services.

use crate::config::Config;
use crate::steps::luks;
use crate::sys::{chroot, quiet, with_stdin, write_file};
use crate::ui::{die, say};

pub fn configure(cfg: &Config, root_dev: &str, swap: &Option<(String, bool)>) {
    say("System configuration");

    write_file("/etc/locale.gen", &format!("en_US.UTF-8 UTF-8\n{}.UTF-8 UTF-8\n", cfg.locale));
    write_file("/etc/locale.conf", &format!("LANG={}.UTF-8\n", cfg.locale));
    write_file("/etc/vconsole.conf", &format!("KEYMAP={}\n", cfg.keymap));
    write_file("/etc/hostname", &format!("{}\n", cfg.hostname));
    write_file("/etc/hosts", &format!(
        "127.0.0.1 localhost\n::1 localhost\n127.0.1.1 {h}.localdomain {h}\n", h = cfg.hostname
    ));

    write_file("/etc/sysctl.d/80-gamecompatibility.conf",
        "vm.max_map_count = 2147483642\nvm.swappiness = 10\n");
    // 99- beats the 50- default, so core dumps never write to (possibly slow) disk
    write_file("/etc/sysctl.d/99-no-core-dumps.conf", "kernel.core_pattern = |/bin/false\n");

    let root_uuid = crate::steps::filesystems::root_uuid(root_dev);
    write_file("/etc/default/grub", &grub_default(cfg, &root_uuid));

    let hibernate = matches!(swap, Some((_, true)));
    if cfg.luks || hibernate {
        // one HOOKS= only: a second drop-in would replace the first
        write_file("/etc/mkinitcpio.conf.d/10-hooks.conf", &hooks_line(cfg, hibernate));
    }
    if cfg.gpu.nvidia() {
        write_file("/etc/mkinitcpio.conf.d/10-nvidia.conf",
            "MODULES=(nvidia nvidia_modeset nvidia_uvm nvidia_drm)\n");
        write_file("/etc/systemd/system/systemd-udevd.service.d/nvidia.conf",
            "[Service]\nPrivateMounts=no\n");
    }

    chroot(&["ln", "-sf", &format!("/usr/share/zoneinfo/{}", cfg.tz), "/etc/localtime"]);
    chroot(&["hwclock", "--systohc"]);
    chroot(&["locale-gen"]);
    chroot(&["sysctl", "--system"]);
    chroot(&["mkinitcpio", "-P"]);
    chroot(&["grub-install", "--target=x86_64-efi", "--efi-directory=/boot", "--bootloader-id=GRUB"]);
    chroot(&["grub-mkconfig", "-o", "/boot/grub/grub.cfg"]);
    // the boot menu may be wrong, but a missing EFI entry is the machine that does not boot
    if !crate::sys::exists(&format!("{}/boot/EFI/GRUB/grubx64.efi", crate::sys::MNT)) {
        die("grub-install wrote nothing to the ESP, check /boot is really the EFI partition");
    }

    users(cfg);
    services(cfg);
}

fn users(cfg: &Config) {
    say("Users");
    // the login shell has to exist in the target; useradd accepts any path without checking
    if !crate::sys::exists(&format!("{}{}", crate::sys::MNT, cfg.shell)) {
        crate::ui::die(&format!("{} is not in the target, login shell would be broken", cfg.shell));
    }
    quiet("arch-chroot", &[crate::sys::MNT, "useradd", "-m", "-G", "wheel", "-s", &cfg.shell, &cfg.user]);
    if !crate::sys::exists(&format!("{}/home/{}", crate::sys::MNT, cfg.user)) {
        crate::ui::die(&format!("useradd failed for {}", cfg.user));
    }
    with_stdin(&format!("{}:{}\n", cfg.user, cfg.user_pass), "arch-chroot", &[crate::sys::MNT, "chpasswd"]);
    with_stdin(&format!("root:{}\n", cfg.root_pass), "arch-chroot", &[crate::sys::MNT, "chpasswd"]);
    write_file("/etc/sudoers.d/10-wheel", "%wheel ALL=(ALL:ALL) ALL\n");
    quiet("chmod", &["440", &format!("{}/etc/sudoers.d/10-wheel", crate::sys::MNT)]);
    quiet("chmod", &["700", &format!("{}/root", crate::sys::MNT)]);
}

fn services(cfg: &Config) {
    say("Services");
    chroot(&["systemctl", "enable", "NetworkManager", "systemd-timesyncd", "fstrim.timer"]);
    // the mirrorlist came from the ISO; keep it fresh when reflector is installed
    if cfg.packages.contains(&"reflector") {
        write_file("/etc/xdg/reflector/reflector.conf", &crate::sys::reflector_conf());
        quiet("arch-chroot", &[crate::sys::MNT, "systemctl", "enable", "reflector.timer"]);
    }
}

fn hooks_line(cfg: &Config, hibernate: bool) -> String {
    let encrypt = if cfg.luks { "encrypt " } else { "" };
    let resume = if hibernate { "resume " } else { "" };
    format!(
        "HOOKS=(base udev autodetect microcode modconf kms keyboard keymap consolefont block {}{}filesystems fsck)\n",
        encrypt, resume
    )
}

fn grub_default(cfg: &Config, root_uuid: &str) -> String {
    let mut cmdline = String::from("loglevel=3 quiet rw");
    let rootflags = if cfg.fs.is_btrfs() { " rootflags=subvol=@" } else { "" };
    let fstype = format!(" rootfstype={}", cfg.fs.key());
    if cfg.luks {
        cmdline.push_str(&format!(
            " cryptdevice=UUID={}:{} root=/dev/mapper/{}{}{}",
            root_uuid, luks::MAPPER, luks::MAPPER, fstype, rootflags
        ));
    } else {
        cmdline.push_str(&format!(" root=UUID={}{}{}", root_uuid, fstype, rootflags));
    }

    if cfg.gpu.nvidia() {
        cmdline.push_str(" nvidia_drm.modeset=1 nvidia.NVreg_PreserveVideoMemoryAllocations=1");
    }
    format!(
        "GRUB_DEFAULT=0\nGRUB_TIMEOUT=3\nGRUB_DISTRIBUTOR=\"Arch\"\n\
         GRUB_CMDLINE_LINUX_DEFAULT=\"{}\"\nGRUB_CMDLINE_LINUX=\"\"\n\
         GRUB_TERMINAL=console\nGRUB_DISABLE_OS_PROBER=false\n",
        cmdline
    )
}

#[cfg(test)]
mod tests {
    use super::{grub_default, hooks_line};
    use crate::config::Config;
    use crate::{detect, fs};

    fn cfg(f: fs::Fs, luks: bool) -> Config {
        Config {
            disk: "/dev/sda".into(),
            dual: false,
            esp_part: None,
            parts: Vec::new(),
            luks,
            multilib: false,
            fs: f,
            gpu: detect::Gpu::Virt,
            hostname: "host".into(),
            tz: "UTC".into(),
            keymap: "us".into(),
            locale: "en_US".into(),
            user: "user".into(),
            shell: "/bin/bash".into(),
            luks_pass: String::new(),
            user_pass: String::new(),
            root_pass: String::new(),
            packages: Vec::new(),
            hibernate: false,
        }
    }

    /// A wrong word here is an unbootable system, so pin the xfs and btrfs shapes.
    #[test]
    fn grub_line_matches_the_filesystem() {
        let xfs = grub_default(&cfg(fs::Fs::Xfs, false), "1111");
        assert!(xfs.contains("root=UUID=1111 rootfstype=xfs"));
        assert!(!xfs.contains("rootflags="));
        let btrfs = grub_default(&cfg(fs::Fs::Btrfs, false), "1111");
        assert!(btrfs.contains("rootfstype=btrfs rootflags=subvol=@"));
        let luks = grub_default(&cfg(fs::Fs::Ext4, true), "1111");
        assert!(luks.contains("cryptdevice=UUID=1111:root root=/dev/mapper/root rootfstype=ext4"));
    }

    /// encrypt unlocks the device, resume must come after it or the swap is not readable yet.
    #[test]
    fn hooks_put_encrypt_before_resume() {
        let hooks = hooks_line(&cfg(fs::Fs::Btrfs, true), true);
        let (enc, res) = (hooks.find("encrypt").unwrap(), hooks.find("resume").unwrap());
        assert!(enc < res, "{}", hooks);
    }
}
