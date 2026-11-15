# AGENTS.md

## Safe verification

- Never run `./archust_install.sh` without `--check` outside a root Arch live ISO. Normal mode
  selects a disk, writes partitions and formats filesystems after confirmation.
- Fast safe gate: `cargo test && cargo clippy --all-targets && ./archust_install.sh --check`.
- Build release binary: `cargo build --release --locked`.
- Do not run `cargo fmt` wholesale: the repository baseline is not rustfmt-clean and it creates an
  unrelated whole-tree diff. Preserve surrounding formatting for targeted edits.

## Runtime and structure

- `src/main.rs` wires the install order: live-ISO guard, mirrors, prompts, final confirmation,
  then `steps::install`.
- `src/prompt.rs` owns answers, package selection and interactive picker behavior. `src/disk.rs`
  owns disk discovery and partition creation. `src/steps/` performs destructive work.
- `FLOW.md` maps source-level commands and required order. Update it whenever a command, order or
  fatal/ignored failure behavior changes. Rust code remains the source of truth.
- System picker choices come from the running Arch ISO (`/usr/share/zoneinfo`,
  `/usr/share/kbd/keymaps`, `/etc/locale.gen`); do not validate their full interactive behavior on
  a non-Arch host.

## Build and dependency gotcha

- `archust_install.sh` rebuild detection checks only `src/` and `Cargo.toml`, not `Cargo.lock`.
  After changing dependency resolution or lockfile-only content, run `cargo build --release --locked`
  directly or remove `target/release/archust-install` before testing the launcher.

## Docs, assets and releases

- Keep `README.md`, `FLOW.md` and `CHANGELOG.md` aligned with executable behavior; do not claim a
  live-ISO or hardware result without that test.
- SVG assets are dark/light pairs referenced with GitHub mode fragments in `README.md`. Update both
  variants and their README references together.
- Release every shipped change: update `Cargo.toml` version and move documented changes from
  `CHANGELOG.md` `Unreleased` into a dated release heading. Create matching `vX.Y.Z` tag on the
  release commit; publish with `git push origin main --follow-tags`.
