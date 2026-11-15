//! Everything the user answered, passed down to the install steps.

/// Power management choice. `Profiles` names the initial power-profiles-daemon plan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Power {
    Off,
    Profiles(&'static str),
    Tlp,
}

impl std::fmt::Display for Power {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Power::Off => f.write_str("none"),
            Power::Profiles(profile) => write!(f, "power-profiles-daemon: {}", profile),
            Power::Tlp => f.write_str("tlp: laptop battery saving"),
        }
    }
}

/// Filesystem written to an extra disk after its data is destroyed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExtraFs {
    None,
    Ext4,
    Btrfs,
    Xfs,
    Exfat,
    Fat32,
}

impl std::fmt::Display for ExtraFs {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ExtraFs::None => f.write_str("none (leave the disk raw)"),
            ExtraFs::Ext4 => f.write_str("ext4"),
            ExtraFs::Btrfs => f.write_str("btrfs"),
            ExtraFs::Xfs => f.write_str("xfs"),
            ExtraFs::Exfat => f.write_str("exFAT"),
            ExtraFs::Fat32 => f.write_str("FAT32"),
        }
    }
}

/// An extra disk is erased before the install disk is touched. Zeroing and encryption are
/// optional; the filesystem is created on the single data partition.
pub struct ExtraDisk {
    pub dev: String,
    pub zero: bool,
    pub fs: ExtraFs,
    pub luks: bool,
}

pub struct Config {
    pub disk: String,
    pub dual: bool,
    pub esp_part: Option<String>,
    pub format_esp: bool,
    pub zero_install: bool,
    pub extras: Vec<ExtraDisk>,
    pub extra_luks_pass: String,
    pub parts: Vec<crate::layout::Part>,
    pub luks: bool,
    pub multilib: bool,
    pub fs: crate::fs::Fs,
    pub gpu: crate::detect::Gpu,
    pub bluetooth: bool,
    pub firewall: bool,
    pub cups: bool,
    pub power: Power,
    pub hostname: String,
    pub tz: String,
    pub keymap: String,
    pub locale: String,
    pub user: String,
    pub shell: String,
    pub luks_pass: String,
    pub user_pass: String,
    pub root_pass: String,
    pub packages: Vec<&'static str>,
    /// Hibernation was asked for; the swap device is only known once the partitions exist.
    pub hibernate: bool,
}
