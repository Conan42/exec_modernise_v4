#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
elf="${project_root}/target/riscv64gc-unknown-none-elf/debug/exec-modernise-v4-bonjour"
output="${project_root}/tftp-root/bonjour-jupiter2.bin"
temporary_elf="$(mktemp)"
trap 'rm -f "$temporary_elf"' EXIT

cargo build \
    --manifest-path "${project_root}/Cargo.toml" \
    -p exec-modernise-v4-bonjour \
    --features board \
    --target riscv64gc-unknown-none-elf

# U-Boot receives a flat binary. llvm-objcopy rejects the high 64-bit entry
# address when writing binary output, so clear only the ELF entry metadata in
# a temporary copy; section VMAs remain unchanged in the ELF used for debug.
cp "$elf" "$temporary_elf"
rust-objcopy --change-start=-0x140000000 "$temporary_elf"
rust-objcopy -O binary "$temporary_elf" "$output"

stat -c 'Bonjour image: %n (%s octets)' "$output"
