#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

# Git commit (if message provided, allow no-op when nothing to commit)
if [ -n "${1:-}" ]; then
    git add -A
    git commit -m "$1" || true
fi

# Git push (skip docker compose)
~/apps/.launch.sh --git-only

# Build release binary
CARGO_TARGET_DIR="$(pwd)/tgt" cargo build --release -p kerai-cli

# Install binary (kerai in /usr/local/bin is chown'd to primal:staff)
cp tgt/release/kerai /usr/local/bin/kerai

# Restart — launchd auto-restarts via KeepAlive
kill $(pgrep -f '/usr/local/bin/kerai serve') 2>/dev/null || true
