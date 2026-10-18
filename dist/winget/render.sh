#!/usr/bin/env bash
# Copyright 2026 eat-pray-ai & OpenWaygate
# SPDX-License-Identifier: Apache-2.0
#
# Renders the winget manifest templates for one release into
# OUT_DIR/manifests/e/eat-pray-ai/yutu-rs/VERSION, the layout of
# microsoft/winget-pkgs, taking the installer hashes from SHA256SUMS.
#
# Usage: dist/winget/render.sh VERSION SHA256SUMS OUT_DIR [REPO]

set -euo pipefail

VERSION="${1:?usage: render.sh VERSION SHA256SUMS OUT_DIR [REPO]}"
SUMS="${2:?usage: render.sh VERSION SHA256SUMS OUT_DIR [REPO]}"
OUT_DIR="${3:?usage: render.sh VERSION SHA256SUMS OUT_DIR [REPO]}"
REPO="${4:-eat-pray-ai/yutu}"

sum_of() {
  local found
  found="$(awk -v f="$1" '$2 == f || $2 == "*" f {print toupper($1)}' "${SUMS}")"
  if [[ -z "${found}" ]]; then
    echo "missing from ${SUMS}: $1" >&2
    exit 1
  fi
  echo "${found}"
}

SHA256_X64="$(sum_of yutu-windows-amd64.exe)"
SHA256_ARM64="$(sum_of yutu-windows-arm64.exe)"
RELEASE_DATE="$(date -u +%Y-%m-%d)"

dest="${OUT_DIR}/manifests/e/eat-pray-ai/yutu-rs/${VERSION}"
mkdir -p "${dest}"
for template in "$(dirname "$0")"/eat-pray-ai.yutu-rs*.yaml; do
  sed -e "s|\${VERSION}|${VERSION}|g" \
    -e "s|\${REPO}|${REPO}|g" \
    -e "s|\${RELEASE_DATE}|${RELEASE_DATE}|g" \
    -e "s|\${SHA256_X64}|${SHA256_X64}|g" \
    -e "s|\${SHA256_ARM64}|${SHA256_ARM64}|g" \
    "${template}" >"${dest}/$(basename "${template}")"
done
echo "rendered ${dest}"
