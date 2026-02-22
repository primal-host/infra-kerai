#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

# Git push (and commit if message provided) — skip docker compose
~/apps/.launch.sh --git-only "$@"

# Build release binary
CARGO_TARGET_DIR="$(pwd)/tgt" cargo build --release -p kerai-cli

# Install binary
cp tgt/release/kerai /usr/local/bin/kerai

# Restart — launchd auto-restarts via KeepAlive
kill $(pgrep -f '/usr/local/bin/kerai serve') 2>/dev/null || true
