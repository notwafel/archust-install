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
    if !fs::read_to_string("/proc/self/status").unwrap_or_default().contains("Uid:\t0") {
        die("run as root");
    }
}

pub const REFLECT_COUNTRIES: &str =
    "Germany,France,Netherlands,Belgium,Luxembourg,Austria,Switzerland,Denmark,Sweden,Finland,\
     Norway,Iceland,Ireland,Italy,Spain,Portugal,Poland,Czechia,Slovakia,Slovenia,Hungary,Croatia,\
     Estonia,Latvia,Lithuania,Romania,Bulgaria,Greece,United States,United Kingdom,Canada,\
     Australia,New Zealand,Japan";

/// One flag per line, as `/etc/xdg/reflector/reflector.conf` expects.
pub fn reflector_conf() -> String {
    format!(
        "--country {}\n--protocol https\n--score 100\n--sort rate\n--fastest 10\n--completion-percent 100\n--save /etc/pacman.d/mirrorlist\n",
        REFLECT_COUNTRIES
    )
}

/// The same settings as a command line: reflector has no `--config`, only the systemd unit
/// reads reflector.conf, so the installer has to pass the flags itself.
fn reflector_args() -> Vec<String> {
    let mut args = vec!["--country".to_string(), REFLECT_COUNTRIES.to_string()];
    for (flag, value) in [
        ("--protocol", "https"),
        ("--score", "100"),
        ("--sort", "rate"),
        ("--fastest", "10"),
        ("--completion-percent", "100"),
        ("--save", "/etc/pacman.d/mirrorlist"),
    ] {
        args.push(flag.to_string());
        args.push(value.to_string());
    }
    args
}

/// 10 fastest HTTPS mirrors from trusted countries, then the keyring.
pub fn prepare_mirrors() {
    crate::ui::say("Mirrors: HTTPS, 10 fastest from trusted countries");
    // live writes go to a tmpfs sized at boot; a full install needs far more than the default
    grow_cowspace();
    const LIVE_CONF: &str = "/etc/xdg/reflector/reflector.conf";
    if let Some(dir) = Path::new(LIVE_CONF).parent() {
        fs::create_dir_all(dir).ok();
    }
    fs::write(LIVE_CONF, reflector_conf()).unwrap_or_else(|e| die(&format!("cannot write {}: {}", LIVE_CONF, e)));
    quiet("pacman", &["-Sy", "--noconfirm", "--needed", "reflector"]);
    let args = reflector_args();
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run("reflector", &refs);
    let n = fs::read_to_string("/etc/pacman.d/mirrorlist")
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("Server = https://"))
        .count();
    if n != 10 {
        die(&format!("expected 10 mirrors, got {}", n));
    }
    // the ISO database can be months old and `-Sy` after a partial refresh breaks deps, so a
    // full upgrade is the only safe refresh; cowspace is what makes it fit
    if !quiet_ok("pacman", &["-Syu", "--noconfirm"]) {
        crate::ui::warn("pacman -Syu failed on the live ISO, trying a plain refresh");
        quiet("pacman", &["-Syy", "--noconfirm"]);
    }
    quiet("pacman", &["-Sy", "--noconfirm", "--needed",
        "archlinux-keyring", "btrfs-progs", "gptfdisk", "dosfstools", "cryptsetup"]);
    quiet("pacman-key", &["--init"]);
    quiet("pacman-key", &["--populate", "archlinux"]);
}

/// remounts the archiso copy-on-write layer bigger; `pacstrap` alone needs a few GB there
pub fn grow_cowspace() {
    const COWSPACE: &str = "/run/archiso/cowspace";
    if !exists(COWSPACE) {
        return;
    }
    let avail = out("df", &["-BK", "--output=avail", COWSPACE])
        .and_then(|s| s.lines().nth(1).map(|v| v.trim().trim_end_matches('K').to_string()))
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
        assert!(conf.lines().count() >= 6);
    }

    /// The command line must carry the same settings; reflector rejects `--config`.
    #[test]
    fn reflector_args_match_the_conf() {
        let args = reflector_args();
        assert!(!args.iter().any(|a| a == "--config"));
        for flag in ["--country", "--protocol", "--score", "--sort", "--fastest", "--save"] {
            assert!(args.iter().any(|a| a == flag), "missing {}", flag);
        }
        assert_eq!(args[0], "--country");
        assert_eq!(args.len() % 2, 0, "every flag needs a value: {:?}", args);
    }
}
