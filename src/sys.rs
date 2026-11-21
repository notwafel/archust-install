//! Thin wrappers over the programs the installer shells out to.

use crate::ui::{die, warn};
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

/// Same as `chroot`, but returns the combined output with the success flag. For steps whose
/// generic "failed" message hides the hook or module that actually broke, such as `mkinitcpio -P`.
pub fn chroot_log(args: &[&str]) -> (bool, String) {
    let mut all = vec![MNT];
    all.extend_from_slice(args);
    match Command::new("arch-chroot").args(&all).output() {
        Ok(o) => (
            o.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            ),
        ),
        Err(e) => (false, e.to_string()),
    }
}

/// `mkinitcpio -P` exits 0 even when a preset reports "errors were encountered during the build",
/// so the image is incomplete while the exit code looks fine.
pub fn mkinitcpio_errors(log: &str) -> Vec<&str> {
    log.lines()
        .filter(|l| l.contains("ERROR") || l.contains("errors were encountered"))
        .collect()
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
    // the whole pipeline is x86_64-only: grub x86_64-efi, GPT, no BIOS path
    if out("uname", &["-m"]).as_deref() != Some("x86_64\n") {
        die("this installer is x86_64 only");
    }
    // the plan always creates a GPT with an ESP and installs GRUB for x86_64-efi; booted in
    // legacy BIOS mode grub-install fails only after the disk has been wiped
    if !exists("/sys/firmware/efi") {
        die("boot the ISO in UEFI mode (legacy BIOS boot cannot finish this install)");
    }
    // a live system upgraded in place keeps the old kernel running but replaces /usr/lib/modules,
    // so no module (vfat for the ESP, the filesystem drivers) can load later in the install
    let release = out("uname", &["-r"]).unwrap_or_default();
    let release = release.trim();
    if !exists(&format!("/usr/lib/modules/{}", release)) {
        die(&format!(
            "/usr/lib/modules/{} is missing: this live system was upgraded in place - reboot the ISO",
            release
        ));
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

/// HTTPS mirrors from trusted countries, then the refreshed keyring.
pub fn prepare_mirrors() {
    crate::ui::say("Mirrors: 10 latest HTTPS mirrors from trusted countries, sorted by rate");
    // live writes go to a tmpfs sized at boot; a full install needs far more than the default
    grow_cowspace();
    const LIVE_CONF: &str = "/etc/xdg/reflector/reflector.conf";
    if let Some(dir) = Path::new(LIVE_CONF).parent() {
        fs::create_dir_all(dir).ok();
    }
    fs::write(LIVE_CONF, reflector_conf()).unwrap_or_else(|e| die(&format!("cannot write {}: {}", LIVE_CONF, e)));
    // the official ISO already ships every helper; installing one by name from a newer repo
    // would mix it with the running live system and can pull a newer glibc/pacman into tmpfs
    for tool in ["reflector", "mkfs.fat", "btrfs", "cryptsetup"] {
        if !exists(&format!("/usr/bin/{}", tool)) {
            die(&format!("/usr/bin/{} is missing from this ISO - use a current Arch ISO", tool));
        }
    }
    run("pacman-key", &["--init"]);
    run("pacman-key", &["--populate", "archlinux"]);
    // reflector first: the ISO's own mirrorlist can be stale enough that `pacman -Sy` fails
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
    // database refresh only; no `-S` with a named package (partial upgrade of the live system)
    run("pacman", &["-Sy", "--noconfirm"]);
}

/// Resolves every target package before the disk is touched. `-Sp` only prints URLs and
/// installs nothing; a renamed or missing package aborts here, not inside pacstrap.
pub fn check_packages(packages: &[&str], multilib: bool) {
    if multilib {
        crate::steps::pacstrap::enable_multilib();
    }
    // no --needed: it would skip packages already installed live, and those still have to
    // resolve in the repos for pacstrap inside the empty target
    let mut args = vec!["-Sp", "--noconfirm"];
    args.extend_from_slice(packages);
    // the probe used to drop pacman's output, so a stale mirror or a repo error was reported as
    // a missing package; only `-Sp` errors are about packages, everything else keeps its message
    let o = Command::new("pacman").args(&args).output().unwrap_or_else(|e| die(&format!("pacman: {}", e)));
    let stderr = String::from_utf8_lossy(&o.stderr);
    if !o.status.success() {
        for line in stderr.lines().filter(|l| l.starts_with("error:")) {
            warn(line);
        }
        // a package that simply is not in a repo gets it named; repo and mirror trouble does not
        if missing_package(&stderr) {
            die("a package is not in the repos, see the pacman error above");
        }
        die("cannot resolve the package list, see the pacman errors above");
    }
}

/// `-Sp` failed because of a package name, not a mirror or a repo database.
fn missing_package(stderr: &str) -> bool {
    stderr.contains("target not found") || stderr.contains("was not found")
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
    if avail >= 2 * 1024 * 1024 * 1024 {
        return;
    }
    crate::ui::say("cowspace: remounting /run/archiso/cowspace with size=4G");
    if !quiet_ok("mount", &["-o", "remount,size=4G", COWSPACE]) {
        crate::ui::warn("could not grow cowspace, pacstrap may run out of space");
    }
}

/// Loads a kernel module; false when it is not available for the running kernel. Only stdout is
/// dropped: modprobe's own stderr names the missing module directory, and the caller's message
/// alone cannot tell a partially upgraded live system from a broken ISO.
pub fn modprobe(module: &str) -> bool {
    std::process::Command::new("modprobe")
        .arg(module)
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
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
    use super::{missing_package, mkinitcpio_errors, reflector_args, reflector_conf};

    /// A missing package must not be confused with a mirror or database failure.
    #[test]
    fn package_errors_are_told_apart_from_repo_errors() {
        assert!(missing_package("error: target not found: nvidia-utills\n"));
        assert!(!missing_package(
            "error: failed retrieving file 'core.db' from mirror.example: Could not resolve host\n"
        ));
    }

    /// A broken preset exits 0, so the exit code alone cannot tell a complete image from a
    /// half-written one.
    #[test]
    fn incomplete_mkinitcpio_build_is_an_error() {
        let log = "==> WARNING: no fsck helper found. fsck will not be run on boot\n\
                   ==> ERROR: Hook 'microcode' cannot be found\n\
                   errors were encountered during the build. The image may not be complete\n";
        assert_eq!(mkinitcpio_errors(log).len(), 2);
        assert!(mkinitcpio_errors("==> WARNING: no fsck helper found.\n").is_empty());
    }

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
