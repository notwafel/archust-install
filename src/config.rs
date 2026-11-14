//! Everything the user answered, passed down to the install steps.

pub struct Config {
    pub disk: String,
    pub dual: bool,
    pub esp_part: Option<String>,
    pub parts: Vec<crate::layout::Part>,
    pub luks: bool,
    pub multilib: bool,
    pub fs: crate::fs::Fs,
    pub gpu: crate::detect::Gpu,
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
    /// Swap partition to resume from: device and whether hibernation was asked for.
    pub swap: Option<(String, bool)>,
}
