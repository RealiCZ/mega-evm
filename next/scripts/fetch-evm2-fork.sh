#!/bin/bash
# Checks out the pinned evm2 fork at `evm2/` next to this repository, for the `next/` workspace's
# path dependency (`../../evm2/crates/evm2`). CI's jobs that build `next/` call this instead of
# repeating the pin read and the checkout; the pin is read from `next/evm2-fork.env` and from
# nowhere else.
#
# Usage: fetch-evm2-fork.sh
#   Run from the repository root. The pin is also written to `$GITHUB_ENV` when that is set, so a
#   later step can read `EVM2_FORK_REPO` / `EVM2_FORK_REV`.
#
# The fork's default branch is not cloned: the pinned commit is fetched by hash, so a pin that was
# never pushed fails with its own message instead of a checkout error. The fork must be public, as
# no credentials are used.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
# shellcheck disable=SC1091
set -a
source "$root/next/evm2-fork.env"
set +a
if [ -z "${EVM2_FORK_REPO:-}" ] || [ -z "${EVM2_FORK_REV:-}" ]; then
  echo "next/evm2-fork.env must set EVM2_FORK_REPO and EVM2_FORK_REV" >&2
  exit 1
fi
if ! printf '%s\n' "$EVM2_FORK_REPO" | grep -Eq '^[^/[:space:]]+/[^/[:space:]]+$'; then
  echo "EVM2_FORK_REPO must be owner/name" >&2
  exit 1
fi
if ! printf '%s\n' "$EVM2_FORK_REV" | grep -Eq '^[0-9a-f]{40}$'; then
  echo "EVM2_FORK_REV must be a 40-character commit sha" >&2
  exit 1
fi
if [ -n "${GITHUB_ENV:-}" ]; then
  printf 'EVM2_FORK_REPO=%s\nEVM2_FORK_REV=%s\n' "$EVM2_FORK_REPO" "$EVM2_FORK_REV" >> "$GITHUB_ENV"
fi

fork=$root/evm2
# Only this script's own checkout is replaced, never a directory outside the repository.
case "$fork" in "$root"/evm2) ;; *) echo "unexpected fork path $fork" >&2; exit 1 ;; esac
rm -rf "${fork:?}"
mkdir -p "$fork"
git -C "$fork" init --quiet
if ! git -C "$fork" fetch --no-tags --quiet --depth=1 \
  "https://github.com/${EVM2_FORK_REPO}.git" "$EVM2_FORK_REV" \
  || ! git -C "$fork" checkout --detach --quiet "$EVM2_FORK_REV"; then
  echo "fork pin not pushed: commit ${EVM2_FORK_REV} is not in ${EVM2_FORK_REPO}. Push it there or update next/evm2-fork.env." >&2
  exit 1
fi
