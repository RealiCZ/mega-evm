#!/bin/bash
# Installs `zstd` when the image does not ship it: the gates extract `.zst` inputs and
# `check-reference.sh` packs them. CI's jobs call this once instead of repeating the install.
#
# Usage: install-zstd.sh
set -euo pipefail

if command -v zstd > /dev/null 2>&1; then
  exit 0
fi
sudo apt-get update
sudo apt-get install -y --no-install-recommends zstd
