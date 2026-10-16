#!/usr/bin/env bash
set -euo pipefail

exec cargo run -p jupiter2ctl -- monitor --serial "${1:-/dev/ttyUSB0}"

