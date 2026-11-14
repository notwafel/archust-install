<div align="center">

![Archust Install](assets/banner-dark.svg#gh-dark-mode-only)
![Archust Install](assets/banner-light.svg#gh-light-mode-only)

![UEFI + GPT, btrfs / ext4 / xfs, LUKS2, GRUB, Wayland, NVIDIA / AMD / Intel](assets/features-dark.svg#gh-dark-mode-only)
![UEFI + GPT, btrfs / ext4 / xfs, LUKS2, GRUB, Wayland, NVIDIA / AMD / Intel](assets/features-light.svg#gh-light-mode-only)

[Run](#run) · [Controls](#controls) · [Questions](#questions) · [What it builds](#what-it-builds) · [Self-check](#self-check) · [Limits](#limits)

</div>

<br>

Arch installer for the live ISO, written in Rust. No `archinstall` - it partitions, formats, encrypts and bootstraps the system itself. Plain Arch, nothing else: base packages, kernel, firmware, drivers, NetworkManager, GRUB. Add whatever you want afterwards.

> [!WARNING]
> **Not tested on real hardware.** The TUI and the install path have not run on a live ISO yet. Try it in a VM first.

![Pipeline: partition, format, LUKS, mount, pacstrap and fstab, configure](assets/pipeline-dark.svg#gh-dark-mode-only)
![Pipeline: partition, format, LUKS, mount, pacstrap and fstab, configure](assets/pipeline-light.svg#gh-light-mode-only)

## Run

On the Arch live ISO, as root, with the network up:

```sh
pacman -S rust git
git clone https://github.com/notwafel/archust-install && cd archust-install
mount -o remount,size=4G /run/archiso/cowspace   # live ISO ships a tiny copy-on-write layer
./archust_install.sh
```

`rust` and `git` are on the live ISO already, so that `pacman -S` does nothing. Never run
`pacman -Sy` or `pacman -Syu` on the live ISO to get them: the booted system lives in a small
tmpfs, a full upgrade overflows it and leaves pacman unable to commit anything.
The installer grows the layer itself if it finds less than 2 GiB free.

`archust_install.sh` is a tiny launcher: it runs `cargo build --release --locked` (only when the sources changed) and then execs the binary. `--locked` means the build never re-resolves dependencies, so it works on a fresh ISO without network access to crates.io.

## Controls

Arrow keys / `j` `k` to move, Enter to accept, space toggles in multi-select lists, `Esc` / `Ctrl-C` aborts. Nothing else throws you out: a wrong answer is re-asked - empty hostname or username, mismatched passwords, disk that is not a whole disk, filesystem too small for the plan.

## Questions

Eight numbered sections, each starting with a header like `[3/8] Sizes`. Bad input is re-asked,
never fatal.

| step | section | what | default |
|---|---|---|---|
| 1 | Disk | pick a whole disk from `lsblk`; the live ISO's own disk is refused | - |
| 2 | Partitioning | keep existing partitions and install into the free space, or wipe | keep, if free space >= 16 GiB |
| 3 | Sizes | swap GiB, root GiB (0 = rest), EFI size when wiping | 4, 0, 1024 MiB |
| 4 | Filesystem and encryption | btrfs / ext4 / xfs, LUKS, then the plan | btrfs, no LUKS |
| 5 | Hardware | kernel, GPU driver | linux, detected |
| 6 | Packages | multilib, Wayland stack, extra groups | all on |
| 7 | System | hostname, username, timezone, keymap, locale, shell | none, none, UTC, us, en_US, bash |
| 8 | Passwords | LUKS passphrase (only with LUKS), user, root | typed twice |

Hibernation is asked right after the sizes and only when swap is at least as large as RAM.

## What it builds

- GPT, UEFI only. Full wipe or dualboot (existing EFI partition reused as `/boot`, new partitions appended after the last one).
- Root filesystem: **btrfs** (subvolumes `@`, `@home`, `@log`, `@pkg`, `@snapshots`, `@swp`, `noatime,compress=zstd:1`), **ext4** or **xfs** (single root, `noatime`). Subvolumes only exist on btrfs; the GRUB line follows the choice (`rootfstype=`).
- Optional **LUKS2** on the root partition: AES-XTS-plain64, 512-bit key, SHA-512, Argon2id. `encrypt` hook goes into the initramfs, one passphrase unlocks root at boot. The passphrase reaches `cryptsetup` on stdin, never on the command line.
- **GRUB** (EFI, `--target=x86_64-efi`) with `GRUB_TERMINAL=console`, `GRUB_TIMEOUT=3`, `loglevel=3 quiet rw`, `os-prober` enabled.
- Nvidia: `nvidia_drm.modeset=1`, `nvidia.NVReg_PreserveVideoMemoryAllocations=1` on the kernel command line, modules in the initramfs, `systemd-udevd` with `PrivateMounts=no`.
- `systemd-timesyncd` + NetworkManager enabled. Bluetooth is not installed.

<details>
<summary><b>Installed packages</b></summary>

<br>

```
base base-devel locales <kernel> linux-headers linux-firmware dkms intel-ucode|amd-ucode
grub efibootmgr dosfstools mtools btrfs-progs
networkmanager sudo man-db bash-completion nano curl git flatpak
pipewire pipewire-pulse pipewire-alsa wireplumber alsa-utils
mesa vulkan-icd-loader libva-utils
+ GPU: vulkan-intel intel-media-driver | vulkan-radeon libva-mesa-driver
       nvidia-dkms / nvidia-open-dkms + nvidia-utils libva-nvidia-driver
+ multilib: lib32-mesa lib32-vulkan-icd-loader lib32-libva lib32-alsa-plugins lib32-gnutls
  (+ lib32-nvidia-utils on NVIDIA)
+ wayland: wayland wayland-protocols xorg-xwayland libxkbcommon xkeyboard-config
  libinput xf86-input-libinput seatd mesa-utils vulkan-tools xdg-desktop-portal xdg-utils
+ xfsprogs (xfs) or e2fsprogs (ext4)
+ firmware group: linux-firmware-intel linux-firmware-realtek linux-firmware-nvidia
+ cryptsetup, only with LUKS
+ os-prober, only in dualboot
```

nano, curl, git, flatpak, pipewire, man-db, graphics and firmware are group choices, and so is
the login shell (`zsh` or `fish` is installed only when picked). Untick a group and its packages are not installed. With the Wayland answer on, `pacman -S gnome` (or `plasma`, `sway`, ...) works right away.

</details>

No desktop environment, no browser, no compositor. Archust Install stops at a system that is ready for one.

### Tweaks

- `/etc/sysctl.d/80-gamecompatibility.conf`: `vm.max_map_count = 2147483642` (Proton/Wine/DXVK
  need it) and `vm.swappiness = 10` (Arch defaults to 60, which pushes RAM to swap too early),
  applied with `sysctl --system` during the install.
- Hibernation: `resume` hook, `RESUME=UUID=` and `resume=UUID=` on the kernel command line -
  written only when swap is at least as large as RAM and the question was answered yes.
- `fstrim.timer` enabled, so SSD TRIM actually runs.
- `/etc/sysctl.d/99-no-core-dumps.conf`: `kernel.core_pattern = |/bin/false` - a crash writes no multi-GB dump to disk.
- multilib: `[multilib]` uncommented in `/etc/pacman.conf`, on the installed system and on the live ISO (so pacstrap can pull the `lib32-*` packages).
- GRUB: `loglevel=3 quiet rw`, `GRUB_TIMEOUT=3`, console terminal.
- `%wheel` gets sudo through `/etc/sudoers.d/10-wheel`.
- Mirrors: `reflector` picks 10 fastest HTTPS mirrors from trusted countries before anything
  else runs. With the `reflector` group pre-ticked, the same config is written to
  `/etc/xdg/reflector/reflector.conf` and `reflector.timer` is enabled, so the mirrorlist stays
  fresh after install.

<details>
<summary><b>Layout of the code</b></summary>

<br>

```
src/main.rs              entry point: --check / --help / run()
src/config.rs            everything the user answered
src/prompt.rs            question flow (disk → sizes → fs → LUKS → kernel → GPU → …)
src/term.rs              dialoguer prompts: select, multi-select, input, password
src/ui.rs                say / warn / die / note
src/sys.rs               shell wrappers, live-ISO check, mirrors
src/disk.rs              lsblk/parted: list disks, free space, ESP, mkpart
src/detect.rs            GPU family and CPU microcode
src/layout.rs            partition arithmetic (pure, self-tested)
src/fs.rs                btrfs / ext4 / xfs: mkfs, mount options, rootfstype
src/steps/
    mod.rs               the pipeline, in order
    filesystems.rs       mkfs per fs type, btrfs subvolumes, mounts, root UUID
    luks.rs              cryptsetup
    pacstrap.rs          multilib, pacstrap, fstab
    configure.rs         locale, hostname, GRUB, initramfs, sysctl, users, services
```

Pipeline: partition → format → LUKS → mount → pacstrap + fstab → configure.

</details>

## Self-check

```sh
./archust_install.sh --check
```

Runs the partition arithmetic on fixed cases (wipe, dualboot, swap, fixed root) and rejection cases (oversized root, tiny disk, EFI under 300 MiB). Fails on overlap, overflow, or a plan that does not end with root. `cargo test` covers the `[multilib]` uncommenting.

## Limits

- **UEFI only**, x86_64.
- Not tested on real hardware. The TUI and the install path have not run on a live ISO yet.
- `nvidia-dkms` needs the `linux-headers` matching your kernel; `nvidia-open-dkms` needs Turing (RTX 20xx) or newer. Older cards: pick `nvidia-dkms`.
- Dualboot needs an existing EFI partition and free space after the last one. No shrinking, no resizing of existing partitions.
- Timezone, keymap and locale lists are short. The keymap list has `uk`, not `gb`.
- No btrfs snapshot tool, no zram, no hibernation resume setup beyond the swap partition.
- Small installer, not general-purpose. Run `--check` after any edit.

<br>

<div align="center">

<sub>Project by <b>notwafel</b></sub>

</div>
