//! Step 2: optional LUKS2 on root. Also used by GRUB for the kernel command line.

use crate::sys::with_stdin;
use crate::ui::say;

pub const MAPPER: &str = "root";

// LUKS2, AES-256-XTS (512-bit key), SHA-512, Argon2id. Passphrase comes on stdin
// (`--key-file -`) so it is never visible in `ps`.
const ARGS: [&str; 12] = [
    "--type", "luks2",
    "--cipher", "aes-xts-plain64",
    "--key-size", "512",
    "--hash", "sha512",
    "--pbkdf", "argon2id",
    "--pbkdf-force-iterations", "3",
];

/// Formats and opens `dev`; returns the mapper device path.
pub fn encrypt(dev: &str, pass: &str) -> String {
    say("LUKS2: aes-xts-plain64, 512-bit key, argon2id");
    let mut format_args = vec!["luksFormat"];
    format_args.extend_from_slice(&ARGS);
    format_args.extend_from_slice(&["--key-file", "-", dev]);
    with_stdin(pass, "cryptsetup", &format_args);
    with_stdin(pass, "cryptsetup", &["open", dev, MAPPER]);
    format!("/dev/mapper/{}", MAPPER)
}
