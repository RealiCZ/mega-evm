# Sourced by gate.sh and check-stamp.sh.
#
# stamp_dir <dir>: one line that changes when any file under <dir> is added, removed, renamed or
# changed in content (sorted relative paths with the SHA-256 of each file's content).
stamp_dir() {
  (cd "$1" && find . -type f | LC_ALL=C sort | while read -r f; do
    printf '%s  %s\n' "$(shasum -a 256 < "$f" | cut -d' ' -f1)" "$f"
  done) | shasum -a 256 | cut -d' ' -f1
}

# stamp_file <file>: SHA-256 of one file's content.
stamp_file() {
  shasum -a 256 < "$1" | cut -d' ' -f1
}
