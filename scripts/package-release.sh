#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

version="${1:-}"
target="${2:-x86_64-unknown-linux-musl}"
binary_path="${3:-$repo_root/result/bin/monlin}"
out_dir="${OUT_DIR:-$repo_root/dist}"

if [[ -z "$version" ]]; then
  version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
fi

if [[ -z "$version" ]]; then
  echo "failed to determine version from Cargo.toml" >&2
  exit 1
fi

if [[ ! -x "$binary_path" ]]; then
  echo "binary not found or not executable: $binary_path" >&2
  exit 1
fi

mkdir -p "$out_dir"

release_root="monlin-v${version}-${target}"
stage_dir="$(mktemp -d)"
trap 'rm -rf "$stage_dir"' EXIT

mkdir -p "$stage_dir/$release_root"
install -m755 "$binary_path" "$stage_dir/$release_root/monlin"
install -m644 README.md "$stage_dir/$release_root/README.md"
install -m644 LICENSE "$stage_dir/$release_root/LICENSE"

raw_binary="$out_dir/${release_root}"
tarball="$out_dir/${release_root}.tar.gz"
checksums="$out_dir/SHA256SUMS"

install -m755 "$binary_path" "$raw_binary"
tar -C "$stage_dir" -czf "$tarball" "$release_root"

(
  cd "$out_dir"
  sha256sum "$(basename "$raw_binary")" "$(basename "$tarball")" > "$(basename "$checksums")"
)

printf 'wrote %s\n' "$raw_binary"
printf 'wrote %s\n' "$tarball"
printf 'wrote %s\n' "$checksums"
