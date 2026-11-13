#!/usr/bin/env bash
# Runs one test binary of the mutation oracle with its memory capped: next/scripts/mutants.sh
# sets this script as cargo's runner for the host target, so `cargo test` starts every test
# binary through it (rustc is not affected).
#
# The cap is MUTANTS_TEST_MEMORY_MB megabytes of data segment (RLIMIT_DATA, which Linux applies
# to heap and anonymous mappings); 0 or unset runs the binary uncapped, as does any other system
# (macOS neither enforces nor accepts the limit). A mutant that loops while allocating would
# otherwise grow until the machine runs out of memory and takes the whole run with it; under the
# cap its test aborts on a failed allocation, and the mutant counts as caught.
#
# Usage: mutants-test-limit.sh <test binary> [args...]
set -u
mb=${MUTANTS_TEST_MEMORY_MB:-0}
case "$mb" in
  "" | *[!0-9]*) echo "MUTANTS_TEST_MEMORY_MB must be a number of megabytes" >&2; exit 2 ;;
esac
if [ "$mb" -gt 0 ] && [ "$(uname -s)" = Linux ]; then
  ulimit -d $((mb * 1024)) || { echo "could not cap the data segment at ${mb} MB" >&2; exit 2; }
fi
exec "$@"
