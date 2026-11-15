#!/bin/sh
# archust_install.sh - builds Archust Install and runs it.
#   ./archust_install.sh          install
#   ./archust_install.sh --check  self-test, no root, no disk
set -eu
cd "$(dirname "$0")"

if [ ! -x target/release/archust-install ] || [ -n "$(find src Cargo.toml Cargo.lock -newer target/release/archust-install 2>/dev/null)" ]; then
	command -v cargo >/dev/null || { echo "cargo not found (pacman -S rust)" >&2; exit 1; }
	cargo build --release --locked
fi
exec ./target/release/archust-install "$@"
