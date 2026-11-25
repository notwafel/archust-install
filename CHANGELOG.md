# Changelog

All notable changes to Archust Install. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [Semantic Versioning](https://semver.org/).

## [0.8.0] - 2026-11-25

### Fixed

- **Dualboot ignored free space before a trailing Windows recovery partition.** The installer only
  looked after the last partition, and a Windows shrink leaves the unallocated space between `C:`
  and the recovery partition. It now finds every unallocated range, picks the largest, and asks how
  many GiB of it to use; swap and root are carved out of that choice. The rest of the range stays
  unallocated, and no existing partition is touched.
- **A 200 MiB shared Windows ESP ran out of space mid-install.** A plain (non-LUKS) install now
  mounts the ESP at `/efi` and keeps `/boot` on the root filesystem, so the ESP only holds GRUB.
  Previously kernels and initramfs images landed on the ESP, `mkinitcpio -P` failed with "No space
  left on device", `bsdtar` reported a write error and `grub-install` could not create
  `/efi/EFI/GRUB`. A LUKS root keeps the old `/boot`-on-ESP layout on purpose; the mount step now
  checks the ESP's free space and stops with the path and the amount to free.
- **User, home directory and sudo setup could be half-done.** `useradd` and both `chmod` calls are
  now fatal instead of silent, the chosen shell is added to `/etc/shells` and read back from
  `/etc/passwd`, so a fish/zsh selection cannot silently fall back to bash.
- **The target could miss NetworkManager with no clear error.** `wpa_supplicant` and
  `wireless-regdb` are now in the base package list, and the configure step checks for the
  NetworkManager binary before enabling the service.

## [0.7.0] - 2026-11-21

### Fixed

- **The installed disk did not show up in the UEFI boot menu on some boards.** `grub-install` now
  passes `--removable`, exactly as `archinstall` does: GRUB is also written to
  `esp/EFI/BOOT/BOOTX64.EFI`, the fallback path every firmware reads after its NVRAM entries are
  gone. The ArchWiki GRUB article names MSI boards as the ones where this is mandatory. The
  post-install check now looks for that file, not only for `EFI/GRUB/grubx64.efi`.

### Changed

- **The "zero overwrite" option is now a real secure erase, chosen per disk.** `dd if=/dev/zero`
  never reached the flash cells a controller keeps in reserve, so a disk wiped that way could still
  hand back old data. The installer now asks the drive itself, in this order:
  - any NVMe disk: `nvme sanitize` with the strongest action `id-ctrl` reports (`--preq`, so a
    success that does not purge user data is refused). When `sanicap` is absent the fallback is
    `nvme format -s 2` (cryptographic erase), then `-s 1` (user data erase);
  - anything else that answers `hdparm -I` - real SATA and USB-SATA bridges alike - gets ATA
    SECURE ERASE, enhanced when the drive advertises it;
  - only devices with neither command (SD cards, USB sticks, some bridges) are overwritten with
    `dd`, and the run says so: that path is not a guarantee on flash media.
- **The sanitize is polled to completion, not fired and forgotten.** `nvme sanitize` is
  asynchronous; `nvme log sanitize` must report `0x0001`. `0x0003` (failed), a status that never
  arrives within an hour, a refused erase password, a failed erase and a security-frozen ATA drive
  all stop the run. A half-erased disk must never be reported as wiped.
- **A failed ATA erase clears its password before stopping.** Otherwise the drive stays locked
  behind `archust-erase`, which is worse than an erased disk: it is unusable.
- **Prompts, summary and help say "secure erase" instead of "overwrite with zeros"**, because that
  is now what happens.

## [0.5.0] - 2026-11-21

Checked against the ArchWiki installation guide and `archinstall`.

### Fixed

- **`mkinitcpio` is named explicitly in the package list.** `archinstall` pins it for the same
  reason: `pacman` otherwise takes whichever initramfs provider the host's `pacman.conf` prefers,
  while the configure step runs `mkinitcpio -P` unconditionally.
- **An unsynchronized live clock is reported before anything is downloaded.** A wrong clock fails
  package signature checks and TLS with messages that name neither.
- **`fstrim.timer` is disabled on a btrfs root.** Periodic TRIM on a CoW filesystem costs more than
  it returns; `archinstall` disables it for btrfs as well.

## [0.4.9] - 2026-11-21

### Fixed

- **`mkinitcpio -P` reported "the image may not be complete" without saying why.** The build exits
  0 for a broken preset, so the installer accepted an incomplete initramfs. Its output is now
  captured, every `ERROR` line is printed, and the step fails.
- **The `HOOKS` drop-in was only written for LUKS or hibernation.** A plain btrfs or ext4 install
  kept the stock `HOOKS`, which lists `microcode`; with `None / other` selected no microcode
  package exists and the initramfs build failed. The drop-in is now always written.
- **A missing `fsck.<type>` helper is fatal right after `pacstrap`.** Before, the target silently
  booted without checking its root filesystem.
- **`[multilib]` no longer copies the live `/etc/pacman.conf` into the target.** The live file
  carries hand-made edits such as `IgnorePkg = linux linux-firmware`, which would have pinned the
  installed system's kernel forever. Only the target's own file is edited.

## [0.4.8] - 2026-11-21

### Fixed

- **A btrfs install had no `btrfs-progs` in the target.** The `pacstrap` package match had no
  btrfs arm, so `mkinitcpio -P` warned `no fsck helper found`, and the installed system had no
  `btrfs` tool to repair a filesystem that would not mount.
- **`mkinitcpio -P` failed without a microcode package.** `HOOKS=()` always listed `microcode`,
  which needs `intel-ucode` or `amd-ucode`; with `None / other` selected neither was installed and
  the initramfs build aborted. The hook is only written when a microcode package is chosen.

## [0.4.7] - 2026-11-21

### Fixed

- **README uses a full upgrade with the kernel pinned.** `pacman -Sy --needed git rust` is a
  partial upgrade: `glibc` and `openssl` must match the repos. The step is now
  `pacman -Syu --needed --ignore linux,linux-firmware git rust`, with `IgnorePkg` in
  `/etc/pacman.conf` as a second guard, so the running kernel keeps its modules and `vfat` loads.

## [0.4.6] - 2026-11-21

### Fixed

- **README install steps no longer upgrade the live system.** `pacman -Syu --noconfirm rust git`
  replaced the running kernel's modules with a newer `linux`, and `vfat` could not load. The steps
  now pin `IgnorePkg = linux linux-firmware` and use `pacman -Sy --needed git rust`.

## [0.4.5] - 2026-11-21

### Fixed

- **A live ISO upgraded in place is refused before any prompt.** After `pacman -Syu` on the live
  system the running kernel no longer has `/usr/lib/modules/<release>`, so `modprobe` fails for
  every module and the ESP mount dies with `unknown filesystem type 'vfat'` after the disk has
  already been zeroed. The guard now stops at startup and names the missing directory.
- **`modprobe` keeps its stderr.** Its error was discarded, so a missing module reported only a
  generic message instead of `module vfat not found in directory`.
- **README install steps no longer upgrade the live system.** `pacman -Syu --noconfirm rust git`
  replaced the running kernel's modules with a newer `linux`, and `vfat` could not load. The steps
  now pin `IgnorePkg = linux linux-firmware` and use `pacman -Sy --needed git rust`.
- **The root mount names its filesystem type.** A probe without `-t` printed `EXT4-fs (…): VFS:
  Can't find ext4 filesystem` before the btrfs mount, which reads like a failure in the log.

## [0.4.4] - 2026-11-21

### Fixed

- **The zero overwrite aborted the install when it reached the end of the disk.** `dd` has no
  natural end on a block device, so a full disk came back as `no space left on device` and was
  reported as a failed step after the whole disk had already been written. `count=<size in MiB>`
  is now passed, so `dd` stops at the end of the device and exits 0.

## [0.4.3] - 2026-11-20

### Fixed

- **The NVIDIA picker prefers the proprietary `nvidia-dkms` and falls back to
  `nvidia-open-dkms`** only when `pacman -Si` cannot find the proprietary module, instead of
  always installing the open one.

## [0.4.2] - 2026-11-20

### Fixed

- **The package precheck no longer dies with `not in the repos`.** Three names did not exist in
  the current repositories: `locales` (always selected; `glibc` already ships `locale-gen` and
  `/etc/locale.gen`), `nvidia-dkms` (replaced by `nvidia-open-dkms`) and `libva-mesa-driver`
  (merged into `mesa`). The GPU picker now offers one NVIDIA option instead of two kernel
  modules with the same userspace packages.
- **The package probe prints pacman's own error** and only says "not in the repos" when pacman
  actually reported a missing target; a broken mirror or repository database now keeps its own
  message.

## [0.4.1] - 2026-11-20

### Fixed

- **`[multilib]` uncommenting no longer activates later repos.** The parser ran past every
  commented section header, so a custom ISO with `#[custom]` and `Server = file:///home/custompkgs`
  ended up enabled and `pacman -Sy` failed on the missing `custom.db`. The block now ends at the
  next header of any kind and only `Key = value` directives lose their `#`.
- **reflector runs before the database refresh.** A stale mirrorlist shipped on the ISO could make
  `pacman -Sy` fail before the mirrors were replaced.
- **A legacy-BIOS boot is refused up front.** The install always ends with
  `grub-install --target=x86_64-efi`, which used to fail only after the disk was wiped. The check
  also requires `x86_64`.
- **A disk with a whole-disk filesystem or LUKS signature needs a typed confirmation.** It has no
  partition table, so it looked empty; the disk menu now prints `whole-disk <fstype>`.
- **The reused ESP is checked for mounts before `mkfs.fat`.** Formatting a mounted ESP pulled the
  filesystem out from under the running system.
- **An open `/dev/mapper/data` or `/dev/mapper/root` stops the run** with the exact
  `cryptsetup close` command instead of formatting over the mapper.
- **`systemctl enable` for Bluetooth, CUPS, UFW, power management and reflector is fatal on
  failure.** It was ignored, so a chosen service could silently stay disabled.
- **`hwclock --systohc` no longer aborts the install** on machines without an RTC.
- **Size prompts reject values above 1,000,000**, which overflowed `GiB * 1024`.
- **Half-typed comments in `pacman.conf` are left alone**: only `Key = value` lines are
  uncommented, so `#Include extra repos` stays a comment.
- **`pacman -Sp` no longer uses `--needed`.** Packages already present on the live system were
  skipped instead of being verified for the empty target.
- **Hybrid laptops with an AMD discrete GPU now get the Intel iGPU drivers too**: companion
  packages are added whenever the detected second GPU differs from the chosen one.
- **`wipefs -a` runs on a fresh data partition** so old exFAT/FAT labels and UUIDs do not leak into
  the new filesystem.
- **The live-ISO disk is always excluded from the picker**, including a remastered ISO whose boot
  medium is mounted elsewhere: a `findmnt` fallback resolves it, and only a real `disk` row is
  accepted, never a `loop` or `rom` device.
- **The install disk is checked for mounts and open mappers right before partitioning**, so a
  kept ESP or an old root that is still mounted aborts instead of being wiped under the running
  system.

## [0.4.0] - 2026-11-15

### Added

- **Extra disks can be erased during the install.** After picking the install disk, a multi-select
  offers every other disk. Each one requires its exact path typed back, then gets one GPT partition
  with `ext4`, `btrfs`, `xfs`, `exFAT`, `FAT32` or no filesystem. Zero overwrite and LUKS2 are
  separate per-disk options; one passphrase covers all encrypted extra disks.
- **The install disk can be overwritten with zeros.** A full wipe offers a default-no zero pass
  before partitioning. It runs after `Start the installation?`, so aborting leaves the disk
  untouched.

### Changed

- Extra disks and the install disk are zeroed with `dd if=/dev/zero bs=1M status=progress
  conv=fsync`. The logical-sector limitation on SSDs is documented and marked with a `shortcut:`
  comment.
- `luks::encrypt` takes the mapper name, so data disks use `data` while the root stays `root`.

## [0.3.0] - 2026-11-15

### Added

- **Bluetooth, firewall, printing and power management are optional picks.** Bluetooth installs
  `bluez`/`bluez-utils` and enables `bluetooth.service`; UFW is configured with
  `DEFAULT_INPUT_POLICY="DROP"`, outgoing traffic allowed and IPv6 enabled; CUPS installs `cups`
  and enables `cups.service`; power management offers `power-profiles-daemon` with a chosen
  initial profile, `tlp`, or none.
- **A non-FAT32 ESP can be formatted in place.** A stale ext4 or unformatted ESP no longer forces
  a full-disk wipe; a separate default-no confirmation schedules `mkfs.fat -F32` after the final
  install confirmation, or sends the user to the wipe branch.

### Fixed

- **A retry after a failed LUKS install still failed with `Partition(s) on /dev/sda are being
  used`.** The wipe guard ignored `TYPE=crypt`, so an open `/dev/mapper/root` looked unused. It now
  aborts and prints the `cryptsetup close` command instead of closing the mapper.
- **The final package list was checked after the disk was wiped.** Only three firmware names were
  verified early. The complete `Config.packages` list now resolves with `pacman -Sp --needed`
  after the prompts and before `Start the installation?`, with `[multilib]` enabled when selected.
- **`ask_name` accepted a leading `-`.** `-rf` reached `useradd` as an option after formatting.
- **A mountpoint with spaces was split into fields.** The disk listing uses `lsblk -P`, so
  `/run/media/user/My Disk` stays one mountpoint in the abort message.
- **The disk menu counted a LUKS mapper as a partition.** Only `TYPE=part` rows are counted.
- **`pacman -Sy` with named packages could partially upgrade the live ISO.** The installer no
  longer installs helpers by name; the official ISO already ships them, and missing binaries stop
  the run before any package is pulled into the live tmpfs.
- **`Esc` at the timezone picker did nothing.** The first fuzzy field now aborts instead of
  returning to itself.
- **Dead configuration removed.** The `RESUME=` file is not written (the current `resume` hook
  reads `resume=` from the kernel command line), and the unused `@swp`/`/swap` subvolume is gone.
- **The keep/wipe branch is tested.** `keep_choice` is a pure decision over the `parted` table,
  total size, ESP presence and ESP filesystem; tests cover enough room, too little room, no ESP
  and a non-vfat ESP.

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
- **NVIDIA DKMS did not build for `linux-lts` or `linux-zen`.** Only `linux-headers` was
  installed, so `nvidia-dkms` and `nvidia-open-dkms` had no headers for the selected kernel.
  The matching header package is installed with the kernel.
- **The launcher ignored a `Cargo.lock` change.** `archust_install.sh` compared only `src/` and
  `Cargo.toml` with the release binary, so a lockfile-only update ran stale code. `Cargo.lock` is
  now part of the rebuild check.

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
