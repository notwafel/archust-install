//! Filesystem choice. What gets created, mounted and put in fstab follows from this.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fs { Btrfs, Ext4, Xfs }

impl Fs {
    pub fn key(self) -> &'static str {
        match self {
            Fs::Btrfs => "btrfs",
            Fs::Ext4 => "ext4",
            Fs::Xfs => "xfs",
        }
    }

    /// Subvolumes only make sense on btrfs. The only real difference between the three here.
    pub fn is_btrfs(self) -> bool { self == Fs::Btrfs }

    pub fn mkfs(self, dev: &str) {
        match self {
            Fs::Btrfs => crate::sys::run("mkfs.btrfs", &["-f", "-L", "archust", dev]),
            Fs::Ext4 => crate::sys::run("mkfs.ext4", &["-F", "-L", "archust", dev]),
            Fs::Xfs => crate::sys::run("mkfs.xfs", &["-f", "-L", "archust", dev]),
        }
    }

    pub fn fstab_options(self) -> &'static str {
        match self {
            Fs::Btrfs => "noatime,compress=zstd:1",
            Fs::Ext4 => "noatime",
            Fs::Xfs => "noatime",
        }
    }
}

impl std::fmt::Display for Fs {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { f.write_str(self.key()) }
}

pub const CHOICES: [Fs; 3] = [Fs::Btrfs, Fs::Ext4, Fs::Xfs];

/// Subvolumes only make sense on btrfs.
pub const SUBVOLS: [&str; 5] = ["@", "@home", "@log", "@pkg", "@snapshots"];
pub const SUBVOL_TARGETS: [&str; 5] =
    ["/", "/home", "/var/log", "/var/cache/pacman/pkg", "/.snapshots"];
