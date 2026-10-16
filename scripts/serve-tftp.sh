#!/usr/bin/env bash
set -euo pipefail

root="${1:-tftp-root}"
bind="${2:-0.0.0.0:69}"
exec cargo run -p jupiter2ctl -- tftp-serve --bind "$bind" --root "$root"

