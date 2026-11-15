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
```

Then mirror preparation starts before disk selection.

### Cowspace

```sh
df -B1 --output=avail /run/archiso/cowspace
mount -o remount,size=4G /run/archiso/cowspace    # only below 2 GiB free
```

The remount failure is a warning, not fatal.

### Keyring, helper packages and mirrors

```sh
pacman-key --init
pacman -Sy --needed --noconfirm archlinux-keyring
pacman-key --populate archlinux
pacman -Sy --needed --noconfirm reflector
reflector \
  --country <trusted-country-list> \
  --protocol https \
  --latest 10 \
  --sort rate \
  --save /etc/pacman.d/mirrorlist
pacman -Sy --needed --noconfirm btrfs-progs gptfdisk dosfstools cryptsetup
pacman -Sy --needed --noconfirm archlinux-keyring
pacman-key --populate archlinux
pacman -Sp --needed --noconfirm \
  linux-firmware-intel linux-firmware-realtek linux-firmware-nvidia
```

The installer writes the same reflector flags to:

```text
/etc/xdg/reflector/reflector.conf
```

It then requires at least one `Server = https://...` line in the new mirrorlist. All commands in
this section except the cowspace remount are fatal on failure.

## 2. Prompt flow

1. **Disk**
   - Determine boot disk:

     ```sh
     lsblk -no PKNAME /run/archiso/bootmnt
     lsblk -no PKNAME -J /dev/disk/by-label/ARCH    # fallback
     ```

   - List choices and inspect selected disk:

     ```sh
     lsblk -dno NAME
     lsblk -dpno NAME,SIZE,MODEL
     lsblk -o NAME,SIZE,FSTYPE,LABEL,MOUNTPOINT <disk>
     blockdev --getsize64 <disk>
     ```

   - The boot disk name returned by `lsblk` is excluded from choices.
   - `blockdev --getsize64 <disk>` supplies the size used by the partition plan. Disk choices
     use bare names from `lsblk`, so the prompt adds `/dev/` exactly once.

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

     The reused ESP must report `vfat`. Any other or empty type aborts before the disk is
     written.
   - Wipe requires the exact disk path typed back. EFI size must be at least 300 MiB.

3. **Sizes**
   - Swap default: 4 GiB.
   - Root default: all remaining usable space.
   - Hibernation prompt appears only when swap is at least detected RAM and at least 1 GiB.

4. **Filesystem and encryption**
   - Filesystem: btrfs, ext4 or xfs.
   - Optional LUKS2.
   - The plan prints before the remaining prompts.

5. **Hardware**

   ```sh
   lspci -nn
   ```

   This selects the default GPU entry. CPU vendor comes from `/proc/cpuinfo` for microcode.

6. **Packages**
   - Multilib default: enabled.
   - Wayland + XWayland default: enabled.
   - Optional groups default: selected. The firmware group follows the GPU: none for a VM,
     Intel/Realtek for Intel and AMD, plus NVIDIA firmware for NVIDIA and nouveau.

7. **System**
   - Hostname and username: lowercase ASCII letters, digits, `-`, `_`; 1–31 characters; no
     leading digit.
   - Timezones come from `/usr/share/zoneinfo`; keymaps from `/usr/share/kbd/keymaps`; UTF-8
     locales from `/etc/locale.gen`.
   - Timezone, keymap, locale and shell use fuzzy search. `Esc` returns to the previous picker.

8. **Passwords**
   - LUKS passphrase only when LUKS is selected.
   - User and root passwords always required.
   - Minimum length: 8 characters. Every password is typed twice.

Before destructive work, the installer prints the package list and asks:

```text
Start the installation? [default: no]
```

## 3. Partition creation

Full wipe first checks:

```sh
findmnt -S <disk>
```

A non-empty result aborts. Then it runs:

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
mkfs.fat -F32 -n ESP <efi-device>    # new EFI only
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
btrfs subvolume create /mnt/@                         # btrfs, then @home @log @pkg @snapshots @swp
umount /mnt                                           # btrfs
mount -o subvol=@,noatime,compress=zstd:1 <root> /mnt # btrfs
modprobe vfat                                         # fatal if the running kernel has no vfat
mount -t vfat <efi-device> /mnt/boot
```

For btrfs, the temporary top-level mount creates all six subvolumes before mounting `@` as root:

```text
@          /
@home      /home
@log       /var/log
@pkg       /var/cache/pacman/pkg
@snapshots /.snapshots
@swp       /swap
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

Any swap creates:

```text
/mnt/etc/mkinitcpio.conf.d/10-resume-device.conf
RESUME=UUID=<swap-uuid>
```

Accepted hibernation also creates:

```text
/mnt/etc/default/grub.d/10-resume.cfg
GRUB_CMDLINE_LINUX_DEFAULT="${GRUB_CMDLINE_LINUX_DEFAULT} resume=UUID=<swap-uuid>"
```

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
```

LUKS or accepted hibernation adds `/mnt/etc/mkinitcpio.conf.d/10-hooks.conf`. Nvidia proprietary
or open adds `/mnt/etc/mkinitcpio.conf.d/10-nvidia.conf` and the `systemd-udevd` drop-in.

Commands run inside `/mnt`:

```sh
arch-chroot /mnt ln -sf /usr/share/zoneinfo/<timezone> /etc/localtime
arch-chroot /mnt hwclock --systohc
arch-chroot /mnt locale-gen
arch-chroot /mnt sysctl --system
arch-chroot /mnt mkinitcpio -P
arch-chroot /mnt grub-install --target=x86_64-efi --efi-directory=/boot --bootloader-id=GRUB
arch-chroot /mnt grub-mkconfig -o /boot/grub/grub.cfg
arch-chroot /mnt systemctl enable NetworkManager systemd-timesyncd fstrim.timer
```

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

This command is run through `quiet(...)`; failure is ignored.

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
- `src/steps/luks.rs`: cryptsetup options.
- `src/steps/pacstrap.rs`: multilib, pacstrap, fstab and resume configuration.
- `src/steps/configure.rs`: target files, bootloader, users and services.
