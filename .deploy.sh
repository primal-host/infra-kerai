#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

# Git commit + push (skip docker compose)
~/apps/.launch.sh --git-only "$@"

# Build release binary
cargo build --release -p kerai-cli

# Install binary
sudo cp target/release/kerai /usr/local/bin/kerai

# Restart launchd service
sudo launchctl kickstart -k system/com.primal.kerai
