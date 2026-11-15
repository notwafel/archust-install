//! The pipeline: each install step in order, in its own module.

pub mod cleanup;
pub mod configure;
pub mod filesystems;
pub mod luks;
pub mod pacstrap;

use crate::config::Config;
use crate::disk;
use crate::ui::say;

pub fn install(cfg: &Config) {
    cleanup::run(cfg);
    say("Partitioning");
    let devices = disk::create(&cfg.disk, &cfg.parts, cfg.dual, cfg.fs.key());
    // the device paths only exist now; guessing them from the plan put resume= on the root
    let swap = disk::swap_device(&cfg.parts, &devices, cfg.hibernate);

    let fs = filesystems::format(cfg, &devices);
    let root = if cfg.luks {
        luks::encrypt(&fs.root_raw, &cfg.luks_pass, luks::MAPPER)
    } else {
        fs.root_raw.clone()
    };
    filesystems::mount(cfg, &fs, &root);
    pacstrap::base(cfg, &swap);
    configure::configure(cfg, &root, &fs.root_raw, &swap);
}
