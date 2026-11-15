# Changelog

All notable changes to Archust Install. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.8] - 2026-11-15

### Fixed

- **Installing beside an existing system put the new partitions at 2 MiB.** `free_start` parsed
  `parted -sm` partition rows as if they started with `/dev/`, found none, and always returned 2.
  The dualboot branch therefore planned the root partition over the existing table. It now reads
  the numeric partition rows and starts after the real last partition.
- **A formatted but inactive swap partition aborted the wipe.** `lsblk` reports `FSTYPE=swap` for
  an unused swap partition too, so the installer ran `swapoff` on a device that was not active.
  Only `[SWAP]` mountpoints are switched off now.
- **The disk picker listed `loop` devices.** The live ISO's squashfs loops and `rom` devices were
  offered as install targets. Only `disk` rows are shown.

## [0.2.7] - 2026-11-15

### Fixed

- **`swapoff sda2 failed: No such file or directory`.** `lsblk -lno` prints bare names, so the
  device path was missing. The listing uses `-p` now and the wipe releases `/dev/sda2`.
- **LUKS installs wrote the wrong `cryptdevice=` UUID.** The GRUB command line used the UUID of
  the filesystem inside the mapper, so the initramfs could not open the container. It now uses
  the UUID of the raw LUKS partition and keeps the mapper UUID for `root=`.
- **cowspace was remounted at the wrong threshold.** The check compared bytes against 2 MiB, not
  2 GiB, so a live ISO with almost no space left skipped the remount.
- **The boot disk could stay in the picker.** The fallback read `lsblk -J` output as plain text,
  so the live disk name was never matched. It now reads `PKNAME` and handles an ISO written
  directly to a disk.

## [0.2.6] - 2026-11-15

### Fixed

- **`parted mklabel` failed with `Partition(s) on /dev/sda are being used`.** The old guard only
  matched a mounted source equal to the whole disk, so a mounted partition or active swap was
  missed. The installer now reads `lsblk -lno NAME,FSTYPE,MOUNTPOINTS`, switches off swap on the
  disk and aborts with the device and mountpoint of anything still mounted.

### Changed

- The disk picker shows size, model, partition count and `in use` per disk instead of a bare
  `lsblk` dump, and says what Enter does.
- The sizes prompts name the recommended values; the root prompt prints how many MiB `0` will
  take.

## [0.2.5] - 2026-11-15

### Changed

- CPU microcode is a prompt (`Intel`, `AMD`, `None / other`) instead of a silent detection. In a
  VM the detected host CPU no longer decides what the guest installs, and `None / other` skips
  microcode entirely.

## [0.2.4] - 2026-11-15

### Fixed

- **Intel-only machines were detected as AMD.** `lspci` matches for `ati` hit the word
  "corporation", so an Intel display controller could be classified as AMD. The check now looks
  for `radeon`.
- **Hybrid laptops got drivers for the NVIDIA card only.** When a second display controller is
  Intel or AMD, its `vulkan-*` and `libva-*` packages are installed next to the NVIDIA set.

## [0.2.3] - 2026-11-15

### Fixed

- **EFI partition mount failed with `unknown filesystem type 'vfat'`.** The installer now loads
  the module with `modprobe vfat` and mounts with an explicit `-t vfat`, so a missing module for
  the running kernel stops the run with that reason instead of a probing error. A reused ESP is
  also checked: a non-FAT32 or unformatted partition aborts before the disk is written.

### Changed

- The firmware group follows the GPU answer: no firmware group for a VM, Intel/Realtek for Intel
  and AMD, plus NVIDIA firmware for NVIDIA and nouveau.

## [0.2.2] - 2026-11-15

### Changed

- README matches the code again: hibernation defaulting to yes, the package list printed once after
  all answers, and the 300 MiB EFI minimum applying to new partitions only.

## [0.2.1] - 2026-11-15

### Fixed

- **Root partition ended at the device size.** The plan used `total - 1` and `disk::create`
  passes `end + 1` to `parted`, so an 80 GiB disk produced `mkpart ... 81920MiB` and parted
  rejected it as outside the device. The root now ends at `total - 2` MiB.

## [0.2.0] - 2026-11-15

### Fixed

- **Disk selection constructed an invalid path.** `lsblk -p` returned `/dev/sda` and the prompt
  prepended `/dev/` again; `blockdev` then failed with `ERR cannot read disk size`. Disk choices
  now use bare names.
- **Partition creation raced `lsblk`.** The installer counted partitions before udev exposed the
  newly created device. It now compares `parted -m` tables and waits for `udevadm settle`.
- **New btrfs installs failed before root mounted.** The `@` subvolume is now created from a
  temporary top-level mount before it is mounted as `/`.
- **zsh and fish installed `bash-completion`.** It is now selected only with bash.
- **Boot after a dualboot install.** The EFI partition was mounted on `/boot` before the root
  filesystem was formatted and mounted; the root mount then hid it, `/boot` on the installed
  system stayed empty and GRUB wrote to the root filesystem. The root is formatted and mounted
  first, then `/boot` is mounted on the real tree.
- **Swap never reached fstab.** `genfstab` ran before `swapon`. The swap partition is now
  activated right after `mkswap`, so it lands in `/etc/fstab`. Hibernation had nothing to
  resume from.
- **`resume=` could point at the root partition.** The swap device was guessed from the plan
  (counting positions in `lsblk`) before the partitions existed. The real device path returned
  by `parted mkpart` is used now; the guess helpers (`partition_index`, `last_number`) are gone.
- **Hibernation parameters in the wrong place.** `RESUME=` is written before `mkinitcpio -P`,
  and `resume=UUID=` is added to the kernel command line through a `/etc/default/grub.d/`
  drop-in instead of a second `GRUB_CMDLINE_LINUX_DEFAULT=`.
- **`pacman -Syu` on the live ISO.** Upgrading the running archiso replaces `pacman`, `glibc` and
  `systemd` inside its own tmpfs and leaves a half-upgraded live system. It only installs the
  helper packages with `pacman -Sy --needed` and stops with the error if that fails.
- **Silent helper installs.** `pacman`, `pacman-key` and `reflector` failures were swallowed;
  they now print their output and stop the install.
- **Package names are checked before the disk is touched.** The extra firmware group
  (`linux-firmware-intel`, `linux-firmware-realtek`, `linux-firmware-nvidia`) is verified with
  `pacman -Sp --needed`; a renamed package stops the run instead of failing inside `pacstrap`.
- **Multilib could be lost in the target.** The copied `/etc/pacman.conf` is read back and the
  install stops if `[multilib]` is still commented out.
- **GRUB install was not verified.** The install stops if `grub-install` wrote nothing to the
  ESP.
- **Weak passwords.** `ask_pass` requires at least 8 characters, still typed twice.
- **Wrong disk key destroyed data with one Enter.** Wiping now asks for the disk name to be
  typed back, and refuses a disk with any mounted partition (`findmnt -S`).
- **The disk picker was a free-text field.** The whole disks are listed with sizes and the live
  ISO's own disk is left out; one wrong path can no longer reach `parted`.
- **Unique selection of the live disk.** `is_live_disk` matched any disk whose name happened to
  match a line of `lsblk` output; the boot disk name is now read once and compared exactly.
- **Root check.** `require_live_iso` read `/proc/self/status` with a substring; it uses
  `id -u` instead.
- **`df` parsing for cowspace.** Byte counts are parsed (`df -B1 --output=avail`) instead of
  trimming a `K` suffix.

### Changed

- Reflector settings live in one table and render both the conf file and the command line;
  the two copies cannot drift apart any more.
- `Fs::rootfstype()` removed: GRUB uses `Fs::key()`, the single place the filesystem name lives.
- `Fs::has_subvolumes()` renamed to `Fs::is_btrfs()`, the only real difference between the three
  filesystems here.
- `disk::free_mib` takes the disk size the caller already read instead of shelling out again;
  `disk::is_whole_disk` and the old text prompt are gone.
- Section headers are numbered (`[2/8] Partitioning`) with a rule under each.

### Added

- Searchable timezone, keymap, locale and shell pickers. They read available choices from the
  live ISO; `Esc` returns one system picker.
- `FLOW.md`, a command-by-command execution map with review targets.
- Tests for the GRUB command line (xfs, btrfs subvolumes, LUKS root) and the initramfs hook
  order (`encrypt` before `resume`).
- `README.md`: theme-safe `<picture>` blocks (light assets only when the reader prefers light,
  dark for everything else, including themes GitHub does not recognise), recovery steps after a
  failed run, refreshed layout, limits and package list.

### Removed

- `pacman -Syu` / `pacman -Syy` fallback on the live ISO.
- `disk::list`, `partition_index`, `disk::last_number`, `Fs::rootfstype`.

## [0.1.0] - 2026-11-14

First working version.

### Added

- Rust installer for the Arch live ISO with its own `parted` / `cryptsetup` / `pacstrap` /
  `arch-chroot` pipeline, no `archinstall`.
- `dialoguer` question flow in eight numbered sections; bad answers are re-asked.
- GPT + UEFI, full wipe or dualboot into the free space after the last partition.
- btrfs (subvolumes `@`, `@home`, `@log`, `@pkg`, `@snapshots`, `@swp`), ext4 and xfs.
- Optional LUKS2 (aes-xts-plain64, 512-bit key, SHA-512, Argon2id), passphrase on stdin.
- GRUB EFI with console output, `os-prober` enabled, `loglevel=3 quiet rw`.
- Package groups: kernel, GPU driver, microcode, multilib, Wayland stack, firmware, extra tools.
- Firmware group: `linux-firmware-intel`, `linux-firmware-realtek`, `linux-firmware-nvidia`.
- Reflector mirror selection plus `reflector.timer` in the target when the group is picked.
- `fstrim.timer`, `vm.max_map_count`, `vm.swappiness = 10`, no core dumps.
- Hibernation setup when swap is at least as large as RAM.
- `archust_install.sh` launcher with `cargo build --release --locked`.
- `--check` layout self-test and `cargo test` unit tests.
- SVG banners and diagrams in `assets/` (dark and light).
- Shell choice limited to bash, zsh and fish; the matching package is installed.
- Cowspace growth before `pacstrap` when the live ISO copy-on-write layer is small.
- Edition 2024, `dialoguer 0.12` as the only dependency.

### Fixed

- A dualboot plan for a fixed root size could run past the end of the disk; the size check
  now rejects it and the question is asked again.
