//! Thin wrappers over the programs the installer shells out to.

use crate::ui::die;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

pub const MNT: &str = "/mnt";

pub fn exists(p: &str) -> bool { Path::new(p).exists() }

/// Captured stdout, or `None` when the program is missing or exits non-zero.
pub fn out(program: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(program).args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).to_string())
}

/// Runs a step attached to the terminal; stops the install on failure.
pub fn run(program: &str, args: &[&str]) {
    if !Command::new(program).args(args).status().map(|s| s.success()).unwrap_or(false) {
        die(&format!("{} {} failed", program, args.join(" ")));
    }
}

/// Silent step; failures are ignored (services that may not exist live, chmod, …).
pub fn quiet(program: &str, args: &[&str]) {
    Command::new(program).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().ok();
}

pub fn chroot(args: &[&str]) {
    let mut all = vec![MNT];
    all.extend_from_slice(args);
    run("arch-chroot", &all);
}

/// Writes `input` to the program's stdin (passwords never appear in `ps`).
pub fn with_stdin(input: &str, program: &str, args: &[&str]) {
    use std::io::Write;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| die(&format!("{}: {}", program, e)));
    child.stdin.take().unwrap().write_all(input.as_bytes()).ok();
    if !child.wait().map(|s| s.success()).unwrap_or(false) {
        die(&format!("{} failed", program));
    }
}

/// Writes a file below `/mnt`, creating parent directories.
pub fn write_file(path: &str, body: &str) {
    let full = format!("{}{}", MNT, path);
    if let Some(dir) = Path::new(&full).parent() {
        fs::create_dir_all(dir).ok();
    }
    fs::write(&full, body).unwrap_or_else(|_| die(&format!("cannot write {}", full)));
}

pub fn require_live_iso() {
    if !exists("/run/archiso") {
        die("run as root on the Arch live ISO");
    }
    if out("id", &["-u"]).as_deref() != Some("0\n") {
        die("run as root");
    }
}

pub const REFLECT_COUNTRIES: &str =
    "Germany,France,Netherlands,Belgium,Luxembourg,Austria,Switzerland,Denmark,Sweden,Finland,\
     Norway,Iceland,Ireland,Italy,Spain,Portugal,Poland,Czechia,Slovakia,Slovenia,Hungary,Croatia,\
     Estonia,Latvia,Lithuania,Romania,Bulgaria,Greece,United States,United Kingdom,Canada,\
     Australia,New Zealand,Japan";

/// reflector settings, written once: the conf file and the command line both come from this.
const REFLECT_FLAGS: [(&str, &str); 5] = [
    ("--country", REFLECT_COUNTRIES),
    ("--protocol", "https"),
    ("--latest", "10"),
    ("--sort", "rate"),
    ("--save", "/etc/pacman.d/mirrorlist"),
];

/// One flag per line, as `/etc/xdg/reflector/reflector.conf` expects.
pub fn reflector_conf() -> String {
    let mut conf = String::new();
    for (flag, value) in REFLECT_FLAGS {
        conf.push_str(&format!("{} {}\n", flag, value));
    }
    conf
}

/// The same settings as a command line: reflector has no `--config`, only the systemd unit
/// reads reflector.conf, so the installer has to pass the flags itself.
fn reflector_args() -> Vec<String> {
    REFLECT_FLAGS.iter().flat_map(|(f, v)| [f.to_string(), v.to_string()]).collect()
}

/// HTTPS mirrors from trusted countries, then the helper packages and refreshed keyring.
pub fn prepare_mirrors() {
    crate::ui::say("Mirrors: 10 latest HTTPS mirrors from trusted countries, sorted by rate");
    // live writes go to a tmpfs sized at boot; a full install needs far more than the default
    grow_cowspace();
    const LIVE_CONF: &str = "/etc/xdg/reflector/reflector.conf";
    if let Some(dir) = Path::new(LIVE_CONF).parent() {
        fs::create_dir_all(dir).ok();
    }
    fs::write(LIVE_CONF, reflector_conf()).unwrap_or_else(|e| die(&format!("cannot write {}: {}", LIVE_CONF, e)));
    // keyring before any package, or reflector and the helpers fail on an old ISO signature
    run("pacman-key", &["--init"]);
    run("pacman", &["-Sy", "--needed", "--noconfirm", "archlinux-keyring"]);
    run("pacman-key", &["--populate", "archlinux"]);
    // no `pacman -Syu`: upgrading the running archiso replaces pacman and glibc inside its own
    // tmpfs and leaves a half-upgraded live system; `-Sy` is enough to install helpers by name
    run("pacman", &["-Sy", "--needed", "--noconfirm", "reflector"]);
    let args = reflector_args();
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run("reflector", &refs);
    let n = fs::read_to_string("/etc/pacman.d/mirrorlist")
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("Server = https://"))
        .count();
    if n == 0 {
        die("reflector wrote no HTTPS mirrors");
    }
    run("pacman", &["-Sy", "--needed", "--noconfirm",
        "btrfs-progs", "gptfdisk", "dosfstools", "cryptsetup"]);
    run("pacman", &["-Sy", "--needed", "--noconfirm", "archlinux-keyring"]);
    run("pacman-key", &["--populate", "archlinux"]);
    // -Sp fails on a name the repos do not know, so a renamed package stops here and not
    // inside pacstrap after the disk is already formatted
    const FIRMWARE: [&str; 3] = ["linux-firmware-intel", "linux-firmware-realtek", "linux-firmware-nvidia"];
    let mut package_args = vec!["-Sp", "--needed", "--noconfirm"];
    package_args.extend(FIRMWARE);
    if out("pacman", &package_args).is_none() {
        die(&format!("not in the repos: {}", FIRMWARE.join(" ")));
    }
}

/// remounts the archiso copy-on-write layer bigger; `pacstrap` alone needs a few GB there
pub fn grow_cowspace() {
    const COWSPACE: &str = "/run/archiso/cowspace";
    if !exists(COWSPACE) {
        return;
    }
    let avail = out("df", &["-B1", "--output=avail", COWSPACE])
        .and_then(|s| s.lines().nth(1).map(|v| v.trim().to_string()))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    if avail >= 2 * 1024 * 1024 {
        return;
    }
    crate::ui::say("cowspace: remounting /run/archiso/cowspace with size=4G");
    if !quiet_ok("mount", &["-o", "remount,size=4G", COWSPACE]) {
        crate::ui::warn("could not grow cowspace, pacstrap may run out of space");
    }
}

/// Loads a kernel module; false when it is not available for the running kernel.
pub fn modprobe(module: &str) -> bool {
    quiet_ok("modprobe", &[module])
}

fn quiet_ok(program: &str, args: &[&str]) -> bool {
    std::process::Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}


#[cfg(test)]
mod tests {
    use super::{reflector_args, reflector_conf};

    /// reflector's config parser wants one flag per line, starting with `--`.
    #[test]
    fn reflector_conf_is_one_flag_per_line() {
        let conf = reflector_conf();
        for line in conf.lines() {
            assert!(line.starts_with("--"), "not a flag line: {:?}", line);
            assert_eq!(line.matches("--").count(), 1, "two flags on one line: {:?}", line);
        }
        assert_eq!(conf.lines().count(), 5);
    }

    /// The command line must carry the same settings; reflector rejects `--config`.
    #[test]
    fn reflector_args_match_the_conf() {
        let args = reflector_args();
        assert!(!args.iter().any(|a| a == "--config"));
        for flag in ["--country", "--protocol", "--latest", "--sort", "--save"] {
            assert!(args.iter().any(|a| a == flag), "missing {}", flag);
        }
        for removed in ["--score", "--fastest", "--completion-percent"] {
            assert!(!args.iter().any(|a| a == removed), "unexpected {}", removed);
        }
        assert_eq!(args[0], "--country");
        assert_eq!(args.len() % 2, 0, "every flag needs a value: {:?}", args);
    }
}
