//! Hardware detection: GPU family, CPU microcode.

use crate::sys::out;
use crate::term::pick;

const CHOICES: [crate::detect::Gpu; 6] = [
    Gpu::Intel, Gpu::Amd, Gpu::Nvidia, Gpu::NvidiaOpen, Gpu::Nouveau, Gpu::Virt,
];

const LABELS: [&str; 6] = [
    "Intel",
    "AMD",
    "NVIDIA proprietary (nvidia-dkms, Turing+)",
    "NVIDIA open kernel module (nvidia-open-dkms, Turing+)",
    "NVIDIA legacy (nouveau)",
    "VM / no dedicated GPU",
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gpu { Intel, Amd, Nvidia, NvidiaOpen, Nouveau, Virt }

impl Gpu {
    pub fn key(self) -> &'static str {
        match self {
            Gpu::Intel => "intel",
            Gpu::Amd => "amd",
            Gpu::Nvidia => "nvidia",
            Gpu::NvidiaOpen => "nvidia-open",
            Gpu::Nouveau => "nouveau",
            Gpu::Virt => "virt",
        }
    }

    pub fn detect() -> Gpu {
        let lspci = out("lspci", &["-nn"]).unwrap_or_default().to_lowercase();
        let gpus: String = lspci
            .lines()
            .filter(|l| l.contains("vga") || l.contains("3d") || l.contains("display"))
            .collect::<Vec<_>>()
            .join("\n");
        if gpus.contains("nvidia") {
            Gpu::Nvidia
        } else if gpus.contains("amd") || gpus.contains("ati") {
            Gpu::Amd
        } else if gpus.contains("intel") {
            Gpu::Intel
        } else {
            Gpu::Virt
        }
    }

    pub fn packages(self) -> &'static [&'static str] {
        match self {
            Gpu::Intel => &["vulkan-intel", "intel-media-driver"],
            Gpu::Amd => &["vulkan-radeon", "libva-mesa-driver"],
            Gpu::Nvidia => &["nvidia-dkms", "nvidia-utils", "libva-nvidia-driver"],
            Gpu::NvidiaOpen => &["nvidia-open-dkms", "nvidia-utils", "libva-nvidia-driver"],
            Gpu::Nouveau | Gpu::Virt => &[],
        }
    }

    pub fn nvidia(self) -> bool {
        matches!(self, Gpu::Nvidia | Gpu::NvidiaOpen)
    }
}

impl std::fmt::Display for Gpu {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { f.write_str(self.key()) }
}

pub fn ask_gpu() -> Gpu {
    let detected = Gpu::detect();
    let default = CHOICES.iter().position(|g| *g == detected).unwrap_or(5);
    crate::ui::say("GPU");
    for (i, label) in LABELS.iter().enumerate() {
        crate::ui::note(&format!("  {}. {}", i + 1, label));
    }
    pick("GPU driver", &CHOICES, default)
}

pub fn memory_gib() -> u64 {
    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let kb: u64 = meminfo
        .lines()
        .find(|l| l.starts_with("MemTotal:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    kb.div_ceil(1048576)
}

pub fn cpu_ucode() -> Option<&'static str> {    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let vendor = cpuinfo.lines().find(|l| l.starts_with("vendor_id"));
    match vendor {
        Some(l) if l.contains("Intel") => Some("intel-ucode"),
        Some(l) if l.contains("AMD") => Some("amd-ucode"),
        _ => None,
    }
}
