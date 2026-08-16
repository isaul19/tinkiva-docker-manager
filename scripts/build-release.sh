#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
cargo clippy --all-targets
cargo test
cargo build --release
printf 'Binario: %s\n' "$ROOT/target/release/tmanager"
ls -lh "$ROOT/target/release/tmanager"
