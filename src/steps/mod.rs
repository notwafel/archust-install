//! The pipeline: each install step in order, in its own module.

pub mod configure;
pub mod filesystems;
pub mod luks;
pub mod pacstrap;

use crate::config::Config;
use crate::disk;
use crate::ui::say;

pub fn install(cfg: &Config) {
    say("Partitioning");
    let devices = disk::create(&cfg.disk, &cfg.parts, cfg.dual, cfg.fs.key());

    let fs = filesystems::format(cfg, &devices);
    let root = if cfg.luks {
        luks::encrypt(&fs.root_raw, &cfg.luks_pass)
    } else {
        fs.root_raw.clone()
    };
    filesystems::mount(cfg, &fs, &root);
    pacstrap::base(cfg);
    configure::configure(cfg, &root);
}
