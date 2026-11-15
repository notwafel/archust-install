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

pub struct Config {
    pub disk: String,
    pub dual: bool,
    pub esp_part: Option<String>,
    pub format_esp: bool,
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
