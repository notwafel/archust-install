# Installer flow

Source-level execution map for current `main`. Commands below are assembled by Rust; values in
`<angle brackets>` come from prompts, detection or the partition plan.

## Rules used by every step

- `run(...)` prints command output and exits on a non-zero status.
- `out(...)` captures stdout and returns `None` when a command fails.
- `quiet(...)` discards stdout/stderr and ignores the exit status.
- `chroot(...)` runs `arch-chroot /mnt <command...>` and exits on failure.
- `with_stdin(...)` writes secrets to stdin, never command-line arguments.
- Target file writes create parent directories below `/mnt`.

Ignored failures are intentional only where `quiet(...)` is shown. Review those carefully.

## 0. Launcher

`archust_install.sh`:

```sh
cd "$(dirname "$0")"
cargo build --release --locked     # only when target/release/archust-install is absent or older than src/ or Cargo.toml
exec ./target/release/archust-install "$@"
```

Modes:

```text
./archust_install.sh           interactive install
./archust_install.sh --check   partition-layout self-check; no root or disk access
./archust_install.sh --help    usage text
```

`--check` evaluates fixed accepted and rejected layout requests. It does not execute installer
commands.

## 1. Live-ISO preflight

The normal install path checks:

```text
/run/archiso exists
id -u == 0
uname -m == x86_64
/sys/firmware/efi exists
```

A legacy-BIOS boot is refused up front: the plan always builds a GPT with an ESP and ends with
`grub-install --target=x86_64-efi`, which would fail only after the disk was wiped.

Then mirror preparation starts before disk selection.

### Cowspace

```sh
df -B1 --output=avail /run/archiso/cowspace
mount -o remount,size=4G /run/archiso/cowspace    # only below 2 GiB free
```

The remount failure is a warning, not fatal.

### Keyring and mirrors

```sh
pacman-key --init
pacman-key --populate archlinux
reflector \
  --country <trusted-country-list> \
  --protocol https \
  --latest 10 \
  --sort rate \
  --save /etc/pacman.d/mirrorlist
pacman -Sy --noconfirm
```

`reflector` runs before the database refresh: the ISO's shipped mirrorlist can be stale enough
that `pacman -Sy` fails on it.

The official ISO already ships `reflector`, `mkfs.fat`, `btrfs` and `cryptsetup`; the installer
checks that the binaries exist and refuses to install any of them by name. A named `pacman -S`
against a newer repository can pull a newer `glibc` or `pacman` into the live tmpfs and leave the
running system half upgraded.

The installer writes the same reflector flags to:

```text
/etc/xdg/reflector/reflector.conf
```

It then requires at least one `Server = https://...` line in the new mirrorlist. All commands in
this section except the cowspace remount are fatal on failure.

The complete target package list is resolved after the prompts, before the disk is touched:

```sh
pacman -Sp --noconfirm <base-packages> <chosen-packages>
```

With multilib selected, `[multilib]` is uncommented in the live `/etc/pacman.conf` and the database
is refreshed first. `-Sp` only prints URLs; a missing or renamed package aborts before
`parted mklabel`. There is no `--needed`: packages already installed on the live system still have
to resolve in the repos for the empty target.

## 2. Prompt flow

1. **Disk**
   - Determine boot disk:

     ```sh
     lsblk -no PKNAME /run/archiso/bootmnt
     lsblk -no PKNAME /dev/disk/by-label/ARCH       # fallback
     lsblk -dno TYPE,NAME /dev/disk/by-label/ARCH   # fallback for a dd'd ISO
     findmnt -no SOURCE /run/archiso/bootmnt | xargs lsblk -no PKNAME   # remastered ISO
     ```

   - List choices and inspect selected disk:

     ```sh
     lsblk -dno TYPE,NAME        # only `disk` rows: loop/rom devices cannot be installed to
     lsblk -dpno NAME,SIZE,MODEL
     lsblk -o NAME,SIZE,FSTYPE,LABEL,MOUNTPOINT <disk>
     blockdev --getsize64 <disk>
     ```

   - The boot disk name returned by `lsblk` is excluded from choices, including the extra-disk
     menu. A disk with no partition table is shown as `whole-disk <fstype>` when the raw device
     carries a filesystem or LUKS signature, so "empty" never hides an old system.
   - `blockdev --getsize64 <disk>` supplies the size used by the partition plan. Disk choices
     use bare names from `lsblk`, so the prompt adds `/dev/` exactly once.
   - After the install disk is picked, a multi-select offers every other disk for erasing. Each
     selected disk needs its exact path typed back, plus a filesystem (`ext4`, `btrfs`, `xfs`,
     `exFAT`, `FAT32`, none), optional zero overwrite and optional LUKS2. One passphrase covers
     all encrypted extra disks. Nothing is erased during the prompts.

2. **Partitioning**
   - Existing layout and ESP detection:

     ```sh
     parted -sm <disk> unit MiB print
     lsblk -no TYPE <disk>
     lsblk -pnlo NAME,PARTTYPENAME <disk>
     ```

   - With existing partitions, dualboot needs an existing `EFI System` partition and at least
     16 GiB after the last partition. Otherwise, the wipe branch is used.

     ```sh
     blkid -s TYPE -o value <efi-device>
     ```

     The reused ESP must report `vfat`. Any other or empty type offers a separate
     `Format <device> as FAT32?` confirmation; declining falls back to the wipe branch with its
     typed confirmation. `mkfs.fat -F32 -n ESP <efi-device>` runs in the filesystem step, after
     the final `Start the installation?` confirmation.
   - Wipe requires the exact disk path typed back. EFI size must be at least 300 MiB. A separate
     default-no question offers to overwrite the whole install disk with zeros before
     partitioning.
   - A disk with no partition table but a whole-disk filesystem or LUKS signature (`blkid -s TYPE
     -o value <disk>`) prints that signature and needs the same typed confirmation; "empty" alone
     never authorizes a wipe.

3. **Sizes**
   - Swap default: 4 GiB.
   - Root default: all remaining usable space; the prompt prints how many MiB that is.
   - EFI default: 1024 MiB, minimum 300 MiB.
   - Hibernation prompt appears only when swap is at least detected RAM and at least 1 GiB.

4. **Filesystem and encryption**
   - Filesystem: btrfs, ext4 or xfs.
   - Optional LUKS2.
   - The plan prints before the remaining prompts.

5. **Hardware**

   ```sh
   lspci -nn
   ```

   `lspci -nn` only picks the default GPU entry and `/proc/cpuinfo` only the default microcode
   entry; both are prompts and can be overridden. `None / other` installs no microcode. On a
   hybrid laptop the GPU picker defaults to NVIDIA; when a second display controller is Intel or
   AMD, its `vulkan-*` and `libva-*` packages are added as well.

6. **Packages**
   - Multilib default: enabled.
   - Wayland + XWayland default: enabled.
   - Optional groups default: selected. The firmware group follows the GPU: none for a VM,
     Intel/Realtek for Intel and AMD, plus NVIDIA firmware for NVIDIA and nouveau.
   - Bluetooth default: off (`bluez`, `bluez-utils`).
   - UFW firewall default: on, `DEFAULT_INPUT_POLICY="DROP"`, output and established traffic
     allowed, IPv6 enabled.
   - CUPS printing default: off (`cups`).
   - Power management default: `power-profiles-daemon` with the `balanced` profile; the picker
     also offers `power-saver`, `performance`, `tlp` or none.

7. **System**
   - Hostname and username: lowercase ASCII letters, digits, `-`, `_`; 1–31 characters; no
     leading digit or `-`.
   - Timezones come from `/usr/share/zoneinfo`; keymaps from `/usr/share/kbd/keymaps`; UTF-8
     locales from `/etc/locale.gen`.
   - Timezone, keymap, locale and shell use fuzzy search. `Esc` returns to the previous picker;
     at the timezone picker it aborts.

8. **Passwords**
   - LUKS passphrase only when LUKS is selected.
   - User and root passwords always required.
   - Minimum length: 8 characters. Every password is typed twice.

Before destructive work, the installer resolves the package list and prints it, then asks:

```text
Start the installation? [default: no]
```

## 3. Partition creation

Before the install disk is touched, the installer erases the extra disks chosen in step 1, then
zeroes the install disk when that option was accepted:

```sh
lsblk -lpno NAME,TYPE,FSTYPE,MOUNTPOINTS -P <disk>   # mounted or open mapper aborts
dd if=/dev/zero of=<disk> bs=1M status=progress conv=fsync   # only when zero was chosen
parted -s <extra-disk> mklabel gpt
parted -s <extra-disk> mkpart data 1MiB 100%
partprobe <extra-disk>
udevadm settle
wipefs -a <extra-partition>                                            # old fs labels and UUIDs
cryptsetup luksFormat --type luks2 ... --key-file - <extra-partition>  # optional
cryptsetup open <extra-partition> data                                  # optional
mkfs.ext4 -F -L data <target>       # or btrfs, xfs, exFAT, FAT32
cryptsetup close data               # optional
```

Extra disks are erased first; a failure there stops the run before the install disk changes.
`mkfs.exfat` must exist on the live ISO, otherwise the run stops before zeroing that disk. LUKS2
on an extra disk is not auto-unlocked; open it with `cryptsetup open` and mount the mapper.
`/dev/mapper/data` or `/dev/mapper/root` left open by an earlier failed run is never closed
automatically: the run stops and prints the `cryptsetup close` command, because formatting over an
open mapper destroys the wrong device.

`dd` overwrites logical sectors only. On an SSD with wear levelling, old NAND cells can survive;
the drive's secure-erase command is the physical option. This is marked with a `shortcut:` comment
in `src/steps/cleanup.rs`.

Full wipe first reads the partitions of the disk:

```sh
lsblk -lpno NAME,TYPE,FSTYPE,MOUNTPOINTS -P <disk>
```

`-P` keeps a mountpoint with spaces in one field. A partition with `FSTYPE=swap` and the
`[SWAP]` mountpoint is active and switched off with `swapoff <device>` (`-p` makes the names
absolute). A `TYPE=crypt` child is an open LUKS mapper: the run aborts and prints
`cryptsetup close <mapper>`; it is never closed automatically. A mounted partition aborts with its
device and mountpoint. Then it runs:

```sh
parted -s <disk> mklabel gpt
```

For each planned partition:

```sh
parted -sm <disk> unit MiB print
parted -s <disk> mkpart <archust-efi|archust-swap|archust-root> <fat32|linux-swap|filesystem> <start>MiB <end-plus-one>MiB
partprobe <disk>
udevadm settle
parted -sm <disk> unit MiB print
parted -s <disk> set <partition-number> esp on    # EFI only
partprobe <disk>                                  # EFI only
udevadm settle                                    # EFI only
```

The second `parted` table must contain exactly one new partition number. `udevadm settle` waits
for device nodes before later format commands. The root partition stops at `total - 2` MiB, so
`end-plus-one` stays below the device size and clear of the backup GPT. Device paths are derived
as:

```text
/dev/sda + 1       = /dev/sda1
/dev/nvme0n1 + 1   = /dev/nvme0n1p1
```

## 4. Filesystems, swap and LUKS

For every planned partition:

```sh
lsblk -lpno NAME,TYPE,FSTYPE,MOUNTPOINTS -P <efi-device>   # formatted ESP must not be mounted
mkfs.fat -F32 -n ESP <efi-device>    # new EFI, or an existing ESP chosen for formatting
mkswap -L swap <swap-device>
swapon <swap-device>
```

LUKS root:

```sh
cryptsetup luksFormat \
  --type luks2 \
  --cipher aes-xts-plain64 \
  --key-size 512 \
  --hash sha512 \
  --pbkdf argon2id \
  --pbkdf-force-iterations 3 \
  --key-file - <root-device>
cryptsetup open <root-device> root
```

The LUKS passphrase goes to stdin for both commands.

Root format and mount:

```sh
mkfs.ext4 -F -L archust <root>                      # ext4
mkfs.xfs -f -L archust <root>                       # xfs
mount -o noatime <root> /mnt                         # ext4 or xfs
mkfs.btrfs -f -L archust <root>                     # btrfs
mount <root> /mnt                                    # btrfs: temporary top-level mount
btrfs subvolume create /mnt/@                         # btrfs, then @home @log @pkg @snapshots
umount /mnt                                           # btrfs
mount -o subvol=@,noatime,compress=zstd:1 <root> /mnt # btrfs
modprobe vfat                                         # fatal if the running kernel has no vfat
mount -t vfat <efi-device> /mnt/boot
```

For btrfs, the temporary top-level mount creates all five subvolumes before mounting `@` as root:

```text
@          /
@home      /home
@log       /var/log
@pkg       /var/cache/pacman/pkg
@snapshots /.snapshots
```

## 5. Pacstrap, multilib, fstab and resume

With multilib selected:

```sh
pacman -Syy --noconfirm
```

The installer uncomments `[multilib]` in the live `/etc/pacman.conf`, later copies it into the
target, and verifies the target copy is not still commented.

Then:

```sh
pacstrap -K /mnt <base-packages> <chosen-packages> <filesystem-package>
genfstab -U /mnt
```

`<filesystem-package>` is `e2fsprogs` for ext4, `xfsprogs` for xfs, nothing extra for btrfs.

Accepted hibernation creates:

```text
/mnt/etc/default/grub.d/10-resume.cfg
GRUB_CMDLINE_LINUX_DEFAULT="${GRUB_CMDLINE_LINUX_DEFAULT} resume=UUID=<swap-uuid>"
```

No `RESUME=` file is written: the current Arch `resume` hook reads `resume=` from the kernel
command line.

## 6. Target configuration

The installer writes, then runs:

```text
/mnt/etc/locale.gen
/mnt/etc/locale.conf
/mnt/etc/vconsole.conf
/mnt/etc/hostname
/mnt/etc/hosts
/mnt/etc/sysctl.d/80-gamecompatibility.conf
/mnt/etc/sysctl.d/99-no-core-dumps.conf
/mnt/etc/default/grub
/mnt/etc/default/ufw                                    # UFW only
/mnt/etc/ufw/ufw.conf                                   # UFW only
/mnt/var/lib/power-profiles-daemon/state.ini            # power profiles only
```

LUKS or accepted hibernation adds `/mnt/etc/mkinitcpio.conf.d/10-hooks.conf`. Nvidia proprietary
or open adds `/mnt/etc/mkinitcpio.conf.d/10-nvidia.conf` and the `systemd-udevd` drop-in.

Commands run inside `/mnt`:

```sh
arch-chroot /mnt ln -sf /usr/share/zoneinfo/<timezone> /etc/localtime
arch-chroot /mnt hwclock --systohc                          # quiet(), warnings ignored
arch-chroot /mnt locale-gen
arch-chroot /mnt sysctl --system
arch-chroot /mnt mkinitcpio -P
arch-chroot /mnt grub-install --target=x86_64-efi --efi-directory=/boot --bootloader-id=GRUB
arch-chroot /mnt grub-mkconfig -o /boot/grub/grub.cfg
arch-chroot /mnt systemctl enable NetworkManager systemd-timesyncd fstrim.timer
arch-chroot /mnt systemctl enable bluetooth.service        # Bluetooth only
arch-chroot /mnt systemctl enable cups.service             # CUPS only
arch-chroot /mnt systemctl enable ufw.service              # firewall only
arch-chroot /mnt systemctl enable power-profiles-daemon.service  # profiles only
arch-chroot /mnt systemctl enable tlp.service              # tlp only
```

Every `systemctl enable` above is fatal on failure: a silently disabled service looks like a working install. `state.ini` contains
`[State]` and `Profile=<balanced|power-saver|performance>` so the chosen plan is active on first
boot; when the live hardware exposes `/sys/firmware/acpi/platform_profile`, it also stores
`PlatformDriver=platform_profile` so power-profiles-daemon does not reject the saved profile.

Required check:

```text
/mnt/boot/EFI/GRUB/grubx64.efi exists
```

User setup:

```sh
arch-chroot /mnt useradd -m -G wheel -s <shell> <user>
arch-chroot /mnt chpasswd    # stdin: <user>:<password>
arch-chroot /mnt chpasswd    # stdin: root:<password>
chmod 440 /mnt/etc/sudoers.d/10-wheel
chmod 700 /mnt/root
```

When the optional reflector group is selected:

```sh
arch-chroot /mnt systemctl enable reflector.timer
```

## 7. Completion and cleanup

On success, the program prints the selected username and asks for unmount/reboot. It does not run
`swapoff`, unmount `/mnt`, close LUKS or reboot.

After a failed run, cleanup is manual:

```sh
swapoff -a
umount -R /mnt
cryptsetup close root    # LUKS only
```

## Review targets

- `src/sys.rs`: live ISO, keyring, mirrors, command wrappers, cowspace.
- `src/prompt.rs`: questions, defaults, package selection and validation.
- `src/disk.rs`: disk discovery, partition arithmetic inputs and destructive partitioning.
- `src/steps/filesystems.rs`: formatting and mount order.
- `src/steps/cleanup.rs`: extra-disk erase, zero overwrite, data partition and LUKS2.
- `src/steps/luks.rs`: cryptsetup options.
- `src/steps/pacstrap.rs`: multilib, pacstrap, fstab and resume configuration.
- `src/steps/configure.rs`: target files, bootloader, users and services.
