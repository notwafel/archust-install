//! Hardware detection: GPU family, CPU microcode. Detection only picks the default; the
//! prompts let the user override it, a VM host CPU says nothing about the target system.

use crate::sys::out;
use crate::term::pick;

const CHOICES: [crate::detect::Gpu; 5] = [
    Gpu::Intel, Gpu::Amd, Gpu::Nvidia, Gpu::Nouveau, Gpu::Virt,
];

// the proprietary kernel module is no longer packaged for Turing+; `nvidia-open-dkms` is the
// only module that builds against the chosen kernel, and `nvidia-utils` still comes with it
const LABELS: [&str; 5] = [
    "Intel",
    "AMD",
    "NVIDIA (nvidia-open-dkms, Turing+)",
    "NVIDIA legacy (nouveau)",
    "VM / no dedicated GPU",
];

const CPU_LABELS: [&str; 3] = [
    "Intel (intel-ucode)",
    "AMD (amd-ucode)",
    "None / other (no microcode)",
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cpu { Intel, Amd, None }

impl Cpu {
    fn detect() -> Cpu {
        let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        let vendor = cpuinfo.lines().find(|l| l.starts_with("vendor_id"));
        match vendor {
            Some(l) if l.contains("Intel") => Cpu::Intel,
            Some(l) if l.contains("AMD") => Cpu::Amd,
            _ => Cpu::None,
        }
    }

    pub fn ucode(self) -> Option<&'static str> {
        match self {
            Cpu::Intel => Some("intel-ucode"),
            Cpu::Amd => Some("amd-ucode"),
            Cpu::None => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gpu { Intel, Amd, Nvidia, Nouveau, Virt }

/// Display-controller lines only: `lspci` also lists Intel audio, USB and SATA chipsets.
fn display_lines() -> String {
    out("lspci", &["-nn"])
        .unwrap_or_default()
        .to_lowercase()
        .lines()
        .filter(|l| l.contains("vga") || l.contains("3d") || l.contains("display"))
        .collect::<Vec<_>>()
        .join("\n")
}

impl Gpu {
    pub fn key(self) -> &'static str {
        match self {
            Gpu::Intel => "intel",
            Gpu::Amd => "amd",
            Gpu::Nvidia => "nvidia",
            Gpu::Nouveau => "nouveau",
            Gpu::Virt => "virt",
        }
    }

    pub fn detect() -> Gpu {
        let gpus = display_lines();
        if gpus.contains("nvidia") {
            Gpu::Nvidia
        } else if gpus.contains("amd") || gpus.contains("radeon") {
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
            // libva-mesa-driver was merged into mesa; nothing to add for VA-API here
            Gpu::Amd => &["vulkan-radeon"],
            Gpu::Nvidia => &["nvidia-open-dkms", "nvidia-utils", "libva-nvidia-driver"],
            Gpu::Nouveau | Gpu::Virt => &[],
        }
    }

    pub fn nvidia(self) -> bool {
        matches!(self, Gpu::Nvidia)
    }
}

impl std::fmt::Display for Gpu {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { f.write_str(self.key()) }
}

/// Second GPU on a hybrid laptop: the integrated one needs its own Vulkan and VA drivers too,
/// otherwise only the discrete card gets them.
pub fn companion() -> Option<Gpu> {
    companion_from(&display_lines())
}

fn companion_from(gpus: &str) -> Option<Gpu> {
    if gpus.contains("intel") {
        Some(Gpu::Intel)
    } else if gpus.contains("amd") || gpus.contains("radeon") {
        Some(Gpu::Amd)
    } else {
        None
    }
}

pub fn ask_cpu() -> Cpu {
    const ORDER: [Cpu; 3] = [Cpu::Intel, Cpu::Amd, Cpu::None];
    let default = ORDER.iter().position(|c| *c == Cpu::detect()).unwrap_or(2);
    let label = pick("CPU microcode", &CPU_LABELS, default);
    ORDER[CPU_LABELS.iter().position(|l| *l == label).unwrap_or(2)]
}

pub fn ask_gpu() -> Gpu {
    let detected = Gpu::detect();
    let default = CHOICES.iter().position(|g| *g == detected).unwrap_or(5);
    let label = pick("GPU driver", &LABELS, default);
    CHOICES[LABELS.iter().position(|l| *l == label).unwrap_or(0)]
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

#[cfg(test)]
mod tests {
    use super::{companion_from, Gpu};

    #[test]
    fn finds_the_integrated_gpu_next_to_nvidia() {
        let intel_and_nvidia = "vga compatible controller: nvidia corporation ga104\nvga compatible controller: intel corporation tigerlake";
        assert_eq!(companion_from(intel_and_nvidia), Some(Gpu::Intel));
        let amd_and_nvidia = "vga compatible controller: advanced micro devices [amd/ati] radeon\n3d controller: nvidia corporation ga107";
        assert_eq!(companion_from(amd_and_nvidia), Some(Gpu::Amd));
        assert_eq!(companion_from("vga compatible controller: nvidia corporation ga104"), None);
    }
}
