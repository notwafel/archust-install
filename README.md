<div align="center">

![Archust Install](assets/banner-dark.svg#gh-dark-mode-only)
![Archust Install](assets/banner-light.svg#gh-light-mode-only)

![UEFI and GPT, btrfs, ext4 and xfs, LUKS2, GRUB, GPU drivers, Wayland](assets/features-dark.svg#gh-dark-mode-only)
![UEFI and GPT, btrfs, ext4 and xfs, LUKS2, GRUB, GPU drivers, Wayland](assets/features-light.svg#gh-light-mode-only)

[Run](#run) · [Header](#header) · [Questions](#questions) · [What it builds](#what-it-builds) · [Recovery](#recovery) · [Limits](#limits)

</div>

<br>

Arch installer for the live ISO, written in Rust. No `archinstall`: it partitions, formats,
encrypts and bootstraps the system itself. It installs a plain Arch base, then prompts for the
kernel, GPU driver, filesystem, encryption and preselected optional package groups. No desktop,
browser or compositor is installed.

> [!WARNING]
> **Not tested on real hardware.** The TUI and the install path have not run on a live ISO yet. Try it in a VM first.

![Install pipeline: partition, format, LUKS, mount, pacstrap and fstab, configure.](assets/pipeline-dark.svg#gh-dark-mode-only)
![Install pipeline: partition, format, LUKS, mount, pacstrap and fstab, configure.](assets/pipeline-light.svg#gh-light-mode-only)

![](assets/divider-dark.svg#gh-dark-mode-only)
![](assets/divider-light.svg#gh-light-mode-only)

## Run

On the Arch live ISO, as root, with the network up:

```sh
mount -o remount,size=4G /run/archiso/cowspace      # live ISO ships a tiny copy-on-write layer
sudo reflector --country Poland,Germany,Czechia --protocol https --latest 20 --sort rate --save /etc/pacman.d/mirrorlist      # u can use without --country flag
pacman -Syu --noconfirm rust git
git clone https://github.com/notwafel/archust-install && cd archust-install
./archust_install.sh
```

`archust_install.sh` builds `target/release/archust-install` with `cargo build --release --locked`
when `src/` or `Cargo.toml` changed, then execs it. `--locked` keeps the committed dependency
versions; first builds can still need crates.io to download uncached dependencies.

The installer refreshes the mirrorlist again before any prompt, with its own trusted-country list and
`--latest 10`. The manual steps above only get `rust`, `git` and the build going.

## Header

Every run shows the same eight numbered sections, so you always know where you are in the list.

```
[2/8] Partitioning
--------------------------------------------------------------
!!! This wipes ALL of /dev/nvme0n1 - irreversible.
Type /dev/nvme0n1 to confirm:
```

Arrow keys / `j` `k` move, Enter accepts, space toggles multi-select lists, and `Ctrl-C`
aborts. In the searchable system pickers, `Esc` returns to the previous field; at the first field
it aborts the run. Invalid hostnames, usernames, passwords and impossible partition sizes are
re-asked. A wrong wipe confirmation aborts the run.

![Controls: arrow keys or j and k to move, Enter to accept, Space to toggle in multi-select lists, Esc or Ctrl-C to abort](assets/controls-dark.svg#gh-dark-mode-only)
![Controls: arrow keys or j and k to move, Enter to accept, Space to toggle in multi-select lists, Esc or Ctrl-C to abort](assets/controls-light.svg#gh-light-mode-only)

![](assets/divider-dark.svg#gh-dark-mode-only)
![](assets/divider-light.svg#gh-light-mode-only)

## Questions

Eight numbered sections, each starting with a header like `[3/8] Sizes`. Validation errors are
re-asked; a wrong typed wipe confirmation aborts.

![Terminal preview of the eight installer sections](assets/terminal-dark.svg#gh-dark-mode-only)
![Terminal preview of the eight installer sections](assets/terminal-light.svg#gh-light-mode-only)

| step | section | what | default |
|---|---|---|---|
| 1 | Disk | select a whole disk, shown with size, model, partition count and `in use`; the live ISO boot disk is excluded; other disks can be erased first with optional zero overwrite, ext4/btrfs/xfs/exFAT/FAT32 and LUKS2 | first listed disk; no extra disks |
| 2 | Partitioning | reuse an existing ESP and install after the last partition, or wipe; a non-FAT ESP offers formatting; a full wipe can overwrite the disk with zeros | keep existing partitions when present; zero off |
| 3 | Sizes | swap GiB, root GiB (`0` = remaining usable space), EFI size for a wipe | `4`, `0`, `1024` MiB |
| 4 | Filesystem and encryption | btrfs / ext4 / xfs, optional LUKS, then show the plan | btrfs, LUKS off |
| 5 | Hardware | kernel, CPU microcode, GPU driver | `linux`, detected CPU, detected GPU |
| 6 | Packages | multilib, Wayland + XWayland, optional groups (firmware follows the GPU), Bluetooth, UFW, CUPS, power management | all groups selected; Bluetooth, CUPS off; UFW and power-profiles-daemon balanced on |
| 7 | System | hostname, username, searchable timezone, keymap, locale and login shell | —, —, UTC, us, en_US, bash |
| 8 | Passwords | LUKS passphrase when selected, user and root passwords | minimum 8 characters, typed twice |

The dualboot branch requires an existing ESP and at least 16 GiB after the last partition. An
empty disk always takes the wipe branch. A new EFI partition must be at least 300 MiB; an existing
one is reused as is. A non-FAT32 ESP can be formatted in place after a separate confirmation, or
the run falls back to the wipe branch.

Other disks can be erased in the same run: each selected disk needs its exact path typed back,
then gets one GPT partition with ext4, btrfs, xfs, exFAT, FAT32 or no filesystem. Zero overwrite and
LUKS2 are separate options per disk; one data-disk passphrase covers all encrypted extras. A full
wipe of the install disk can also zero it first. All erasing happens after `Start the
installation?` and before the install disk is partitioned.

Other disks can be erased in the same run: each selected disk needs its exact path typed back,
then gets one GPT partition with ext4, btrfs, xfs, exFAT, FAT32 or no filesystem. Zero overwrite and
LUKS2 are separate options per disk; one data-disk passphrase covers all encrypted extras. A full
wipe of the install disk can also zero it first. All erasing happens after `Start the
installation?` and before the install disk is partitioned.

Hibernation is offered only when swap is at least RAM and at least 1 GiB. The prompt defaults to
yes, so Enter adds the `resume` hook and `resume=UUID=`. After the last answer, the installer
prints the selected packages and asks `Start the installation?`; its default is no.

Wiping cannot be accepted with Enter: type the disk path exactly. The disk menu shows size, model
and partition count, and marks a disk `in use` when any partition is mounted or swapped on. Before
`parted mklabel`, active swap on that disk is switched off; a mounted partition or an open LUKS
mapper aborts the run with the `cryptsetup close` command to run.

![Safeguards: typed confirmation, mounted source check, live ISO boot disk excluded, ESP verified, multilib read back, firmware names checked](assets/safeguards-dark.svg#gh-dark-mode-only)
![Safeguards: typed confirmation, mounted source check, live ISO boot disk excluded, ESP verified, multilib read back, firmware names checked](assets/safeguards-light.svg#gh-light-mode-only)

![](assets/divider-dark.svg#gh-dark-mode-only)
![](assets/divider-light.svg#gh-light-mode-only)

## What it builds

![Diagram of the intended btrfs layout: EFI, swap, root with optional LUKS2 and subvolumes](assets/layout-dark.svg#gh-dark-mode-only)
![Diagram of the intended btrfs layout: EFI, swap, root with optional LUKS2 and subvolumes](assets/layout-light.svg#gh-light-mode-only)

![Specs: filesystems, LUKS2, GRUB, NVIDIA parameters, services](assets/specs-dark.svg#gh-dark-mode-only)
![Specs: filesystems, LUKS2, GRUB, NVIDIA parameters, services](assets/specs-light.svg#gh-light-mode-only)

<details>
<summary><b>What it builds in detail</b></summary>

<br>

- GPT, UEFI only. Full wipe or dualboot. Dualboot reuses an existing EFI partition as `/boot` and
  creates new partitions after the last existing one.
- **btrfs**: `@`, `@home`, `@log`, `@pkg`, `@snapshots`; mounted with
  `noatime,compress=zstd:1`. **ext4** or **xfs**: one root filesystem mounted with `noatime`.
  `e2fsprogs` or `xfsprogs` is added to `pacstrap` for the selected filesystem.
- Optional **LUKS2** on the root partition: AES-XTS-plain64, 512-bit key, SHA-512 and Argon2id.
  The passphrase reaches `cryptsetup` on stdin, never on the command line.
- **GRUB** EFI: `--target=x86_64-efi`, `GRUB_TERMINAL=console`, `GRUB_TIMEOUT=3`,
  `loglevel=3 quiet rw`, `os-prober` enabled. The install checks for
  `/boot/EFI/GRUB/grubx64.efi` after `grub-install`.
- Nvidia proprietary and open drivers add `nvidia_drm.modeset=1` and
  `nvidia.NVreg_PreserveVideoMemoryAllocations=1`, initramfs modules, and
  `systemd-udevd` with `PrivateMounts=no`.
- NetworkManager, `systemd-timesyncd` and `fstrim.timer` are always enabled. Optional picks enable
  `bluetooth.service`, `ufw.service` (incoming denied, outgoing allowed), `cups.service`, and
  either `power-profiles-daemon.service` with the chosen initial profile or `tlp.service`.

</details>

<details>
<summary><b>Installed packages</b></summary>

<br>

```text
Always:
  base base-devel linux-firmware dkms locales
  grub efibootmgr dosfstools mtools networkmanager sudo
  selected kernel and its headers; selected CPU microcode

Selected GPU:
  Intel: vulkan-intel intel-media-driver
  AMD: vulkan-radeon libva-mesa-driver
  NVIDIA: nvidia-dkms or nvidia-open-dkms, nvidia-utils, libva-nvidia-driver
  nouveau / VM: no additional GPU packages

  Hybrid with NVIDIA (Intel or AMD integrated GPU also present):
  the NVIDIA set plus vulkan-intel intel-media-driver or vulkan-radeon libva-mesa-driver

Selected options:
  multilib: lib32-mesa lib32-vulkan-icd-loader lib32-libva lib32-alsa-plugins lib32-gnutls
            plus lib32-nvidia-utils for NVIDIA proprietary or open
  Wayland: wayland wayland-protocols xorg-xwayland libxkbcommon xkeyboard-config
           libinput xf86-input-libinput seatd mesa-utils vulkan-tools xdg-desktop-portal xdg-utils
  ext4: e2fsprogs
  xfs: xfsprogs
  LUKS: cryptsetup
  dualboot: os-prober
  bash: bash-completion
  zsh / fish: zsh / fish

Selected services:
  Bluetooth: bluez bluez-utils
  Firewall: ufw, deny incoming and allow outgoing
  Printing: cups
  Power: power-profiles-daemon (balanced, power-saver or performance) or tlp

Preselected optional groups:
  Nano editor · curl · git · Flatpak · PipeWire audio · Manual pages · Reflector mirrors
  Graphics: Mesa, Vulkan, VA tools
  Firmware: Intel, Realtek, NVIDIA (an NVIDIA GPU) or Intel, Realtek (Intel, AMD)
  A VM or "no dedicated GPU" gets no Firmware group.
```

The optional groups start selected but can be unticked. `PipeWire audio` adds PipeWire and ALSA
tools. `Graphics` adds `mesa`, `vulkan-icd-loader` and `libva-utils`; it supplies userspace
OpenGL, Vulkan and video-acceleration tooling. `Firmware` adds `linux-firmware-intel` and
`linux-firmware-realtek`, which cover Wi-Fi, Ethernet and Bluetooth chips, plus
`linux-firmware-nvidia` when the GPU is NVIDIA or nouveau. A VM has none of that hardware, so it
gets no firmware group at all.

The selected kernel's headers are installed with it: `linux-headers`, `linux-lts-headers` or
`linux-zen-headers`. That is what `nvidia-dkms` and `nvidia-open-dkms` build against, so NVIDIA
on `linux-lts` or `linux-zen` works without adding headers by hand.

CPU microcode is its own prompt and is independent of the GPU: an AMD CPU with an NVIDIA card
gets `amd-ucode` plus the NVIDIA packages, an Intel CPU with an AMD card gets `intel-ucode` plus
`vulkan-radeon` and `libva-mesa-driver`. `None / other` installs no microcode, which is the right
answer in a VM or on a CPU vendor the packages do not cover.

The installer resolves the complete selected package list against the current repositories after
the prompts and before it touches the disk. A renamed package stops the run before `pacstrap`.

</details>

No desktop environment, no browser, no compositor. Archust Install stops at a system that is ready for one.

### Tweaks

![Tweaks: game compatibility, swappiness, hibernation, SSD TRIM, no core dumps, multilib, sudo, mirrors](assets/tweaks-dark.svg#gh-dark-mode-only)
![Tweaks: game compatibility, swappiness, hibernation, SSD TRIM, no core dumps, multilib, sudo, mirrors](assets/tweaks-light.svg#gh-light-mode-only)

<details>
<summary><b>Tweaks in detail</b></summary>

<br>

- `/etc/sysctl.d/80-gamecompatibility.conf`: `vm.max_map_count = 2147483642` (Proton/Wine/DXVK
  need it) and `vm.swappiness = 10` (Arch defaults to 60, which pushes RAM to swap too early),
  applied with `sysctl --system` during the install.
- Hibernation: when swap is at least RAM and the hibernation prompt is accepted, the initramfs
  gets the `resume` hook and `/etc/default/grub.d/10-resume.cfg` adds `resume=UUID=` to the kernel
  command line. Swap is enabled directly after `mkswap`, before `genfstab` creates `/etc/fstab`.
- `fstrim.timer` enabled, so SSD TRIM actually runs.
- `/etc/sysctl.d/99-no-core-dumps.conf`: `kernel.core_pattern = |/bin/false` - a crash writes no multi-GB dump to disk.
- multilib: `[multilib]` uncommented in `/etc/pacman.conf`, on the installed system and on the live ISO (so pacstrap can pull the `lib32-*` packages). The target copy is read back and the install stops if it is still commented out.
- GRUB: `loglevel=3 quiet rw`, `GRUB_TIMEOUT=3`, console terminal.
- `%wheel` gets sudo through `/etc/sudoers.d/10-wheel`.
- Mirrors: `reflector` selects the 10 latest HTTPS mirrors from trusted countries and sorts them
  by rate before anything else runs. One setting table renders
  `/etc/xdg/reflector/reflector.conf` and its command-line flags; reflector has no `--config`.
  When the optional `reflector` group is selected, the config is written into the target and the
  installer attempts to enable `reflector.timer`.

</details>

![](assets/divider-dark.svg#gh-dark-mode-only)
![](assets/divider-light.svg#gh-light-mode-only)

## Recovery

![Recovery after a failed run: swapoff, umount, cryptsetup close, run the installer again](assets/recovery-dark.svg#gh-dark-mode-only)
![Recovery after a failed run: swapoff, umount, cryptsetup close, run the installer again](assets/recovery-light.svg#gh-light-mode-only)

A failed run leaves `/mnt` mounted, swap active and partitions written. Clean up before trying
again, or the installer starts on a dirty `/mnt`:

```sh
swapoff -a
umount -R /mnt
cryptsetup close root          # only with LUKS
./archust_install.sh
```

Nothing on the disk is rolled back - a rerun formats again, and a wipe starts from an empty
partition table.

<details>
<summary><b>Layout of the code</b></summary>

<br>

```
src/main.rs              entry point: --check / --help / run()
src/config.rs            everything the user answered
src/prompt.rs            question flow (disk -> sizes -> fs -> LUKS -> kernel -> GPU -> ...)
src/term.rs              dialoguer prompts: select, disk picker, multi-select, input, password
src/ui.rs                say / warn / die / note / step / rule / item
src/sys.rs               shell wrappers, live-ISO check, mirrors, keys
src/disk.rs              lsblk/parted: disk menu, busy check, free space, ESP, mkpart
src/detect.rs            GPU family and CPU microcode
src/layout.rs            partition arithmetic (pure, self-tested)
src/fs.rs                btrfs / ext4 / xfs: mkfs, mount options
src/steps/
    mod.rs               the pipeline, in order
    filesystems.rs       mkfs, btrfs subvolumes, swap on, mounts, root UUID
    luks.rs              cryptsetup
    pacstrap.rs          multilib, pacstrap, fstab, resume config
    configure.rs         locale, hostname, GRUB, initramfs, sysctl, users, services
assets/                  banners and diagrams, dark and light
```

Pipeline: partition -> format -> LUKS -> mount -> pacstrap + fstab -> configure.

</details>

![](assets/divider-dark.svg#gh-dark-mode-only)
![](assets/divider-light.svg#gh-light-mode-only)

## Self-check

```sh
./archust_install.sh --check
```

Runs the partition arithmetic on fixed cases (wipe, dualboot, swap, fixed root) and rejection cases (oversized root, tiny disk, EFI under 300 MiB). Fails on overlap, overflow, or a plan that does not end with root. `cargo test` covers the kernel command line (xfs, btrfs, LUKS), the initramfs hook order, the `[multilib]` uncommenting, the two `reflector` renderings, the 1 MiB end margin that keeps `parted` inside the device, the dualboot free-space arithmetic, the `parted`/`lsblk` parsers, the selected kernel headers, and the UTF-8 locale parser.

![Self-check: accepts wipe, dualboot, swap, fixed root; rejects oversized root, tiny disk, EFI under 300 MiB; fails on overlap, overflow, a plan not ending with root](assets/selfcheck-dark.svg#gh-dark-mode-only)
![Self-check: accepts wipe, dualboot, swap, fixed root; rejects oversized root, tiny disk, EFI under 300 MiB; fails on overlap, overflow, a plan not ending with root](assets/selfcheck-light.svg#gh-light-mode-only)

## Limits

- **UEFI only**, x86_64.
- Not tested on real hardware. The TUI and install path have not run on a live ISO yet.
- Timezones and keymaps are read from the live ISO. UTF-8 locales come from `/etc/locale.gen`.
  All three use fuzzy search; `Esc` moves back one system picker and aborts at the first.
- GPU detection chooses a default only. It does not validate NVIDIA hardware compatibility with
  the selected driver package.
- Dualboot needs an existing EFI partition and at least 16 GiB after the last partition. No
  shrinking or resizing of existing partitions. A non-`vfat` ESP must be formatted as FAT32
  first; declining falls back to the wipe branch.
- No snapshot tool, zram or encrypted swap.
- Zero overwrite writes logical sectors. On an SSD with wear levelling, old NAND cells may survive;
  use the drive's secure-erase command when physical erasure is required.
- Small installer, not general-purpose. Run `--check` and `cargo test` after each edit.

![](assets/divider-dark.svg#gh-dark-mode-only)
![](assets/divider-light.svg#gh-light-mode-only)

![Plain Arch. Nothing else. Project by notwafel](assets/footer-dark.svg#gh-dark-mode-only)
![Plain Arch. Nothing else. Project by notwafel](assets/footer-light.svg#gh-light-mode-only)
