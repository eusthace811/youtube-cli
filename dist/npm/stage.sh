#!/usr/bin/env bash
# Copyright 2026 eat-pray-ai & OpenWaygate
# SPDX-License-Identifier: Apache-2.0
#
# Stages the npm packages for `npm pack` or `npm publish`, without touching
# the sources: copies each package to OUT_DIR, stamps VERSION, and puts the
# release binary and LICENSE in it.
#
# Usage: dist/npm/stage.sh VERSION BIN_DIR OUT_DIR [PACKAGE...]
#   VERSION   version without the leading v, e.g. 0.11.0
#   BIN_DIR   directory holding the release assets (yutu-darwin-arm64, ...)
#   PACKAGE   platform packages to stage (default: all six); the wrapper
#             package yutu is always staged

set -euo pipefail

VERSION="${1:?usage: stage.sh VERSION BIN_DIR OUT_DIR [PACKAGE...]}"
BIN_DIR="${2:?usage: stage.sh VERSION BIN_DIR OUT_DIR [PACKAGE...]}"
OUT_DIR="${3:?usage: stage.sh VERSION BIN_DIR OUT_DIR [PACKAGE...]}"
shift 3

NPM_DIR="$(cd "$(dirname "$0")" && pwd)"
LICENSE="${NPM_DIR}/../LICENSE"

# npm package -> release asset. A case, not an associative array, so the
# script runs on macOS's bash 3.2.
asset_of() {
  case "$1" in
    yutu-darwin-arm64) echo "yutu-darwin-arm64" ;;
    yutu-darwin-x64) echo "yutu-darwin-amd64" ;;
    yutu-linux-arm64) echo "yutu-linux-arm64" ;;
    yutu-linux-x64) echo "yutu-linux-amd64" ;;
  esac
}

if [[ $# -eq 0 ]]; then
  set -- yutu-darwin-arm64 yutu-darwin-x64 yutu-linux-arm64 yutu-linux-x64
fi

stage() {
  local pkg="$1" dest="${OUT_DIR}/$1"
  rm -rf "${dest}"
  mkdir -p "${dest}"
  cp -R "${NPM_DIR}/${pkg}/." "${dest}/"
  cp "${LICENSE}" "${dest}/LICENSE"
  sed -i.bak "s/\"0\.0\.0\"/\"${VERSION}\"/g" "${dest}/package.json"
  rm -f "${dest}/package.json.bak"
}

stage yutu
echo "staged ${OUT_DIR}/yutu"

for pkg in "$@"; do
  asset="$(asset_of "${pkg}")"
  if [[ -z "${asset}" ]]; then
    echo "unknown package: ${pkg}" >&2
    exit 1
  fi
  if [[ ! -f "${BIN_DIR}/${asset}" ]]; then
    echo "missing binary: ${BIN_DIR}/${asset}" >&2
    exit 1
  fi
  stage "${pkg}"
  bin="yutu"
  mkdir -p "${OUT_DIR}/${pkg}/bin"
  cp "${BIN_DIR}/${asset}" "${OUT_DIR}/${pkg}/bin/${bin}"
  chmod +x "${OUT_DIR}/${pkg}/bin/${bin}"
  echo "staged ${OUT_DIR}/${pkg} from ${asset}"
done
