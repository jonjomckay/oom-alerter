#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 2 ]; then
  printf 'usage: %s <version> <output-directory>\n' "$0" >&2
  exit 2
fi

version=$1
output_dir=$2
prefix="oom-alerter-${version}/"
archive="${output_dir}/oom-alerter-${version}.tar.gz"

mkdir -p "$output_dir"
git archive --format=tar --prefix="$prefix" HEAD | gzip -n > "$archive"
(cd "$output_dir" && sha256sum "$(basename "$archive")") > "${archive}.sha256"
