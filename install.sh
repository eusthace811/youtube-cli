#!/usr/bin/env bash
# Copyright 2025 eat-pray-ai & OpenWaygate
# SPDX-License-Identifier: Apache-2.0
#
# Installs the yutu release binary for this platform into
# ${XDG_BIN_HOME:-$HOME/.local/bin}, after checking its SHA-256 against the
# release's SHA256SUMS and, when `gh` is available, its build-provenance
# attestation.
#
# Usage: install.sh [--with-skill]
#   --with-skill   also install the youtube agent skill with `npx skills add`
#
# Environment:
#   YUTU_REPO               GitHub repository of the yutu-rs releases, owner/name
#                           (required unless YUTU_RELEASE_URL is set; the release
#                           notes give the command with it filled in)
#   YUTU_RELEASE_URL        base URL of the release assets, any URL curl reads
#                           (default https://github.com/$YUTU_REPO/releases/latest/download)
#   YUTU_SKIP_ATTESTATION=1 skip the attestation check even when gh is present

set -euo pipefail

REPO="${YUTU_REPO:-}"
BASE_URL="${YUTU_RELEASE_URL:-https://github.com/${REPO}/releases/latest/download}"
INSTALL_DIR="${XDG_BIN_HOME:-${HOME}/.local/bin}"
WITH_SKILL=0

for arg in "$@"; do
  case "${arg}" in
    --with-skill) WITH_SKILL=1 ;;
    -h | --help)
      echo "Usage: install.sh [--with-skill]"
      exit 0
      ;;
    *)
      echo "Unknown argument: ${arg}" >&2
      exit 2
      ;;
  esac
done

if [[ -z "${REPO}" && -z "${YUTU_RELEASE_URL:-}" ]]; then
  echo "Set YUTU_REPO to the GitHub repository of the yutu-rs releases (owner/name)." >&2
  echo "The Go project's releases (eat-pray-ai/yutu) are a different program." >&2
  exit 2
fi

ARCH="UNKNOWN"
case $(uname -m) in
  x86_64 | amd64) ARCH="amd64" ;;
  aarch64 | arm64) ARCH="arm64" ;;
esac

if [[ "${ARCH}" == "UNKNOWN" ]]; then
  echo "Unsupported architecture: $(uname -m)"
  echo "Report: https://github.com/${REPO}/issues"
  exit 1
fi

OS="UNKNOWN"
case $(uname -s) in
  Linux) OS="linux" ;;
  Darwin) OS="darwin" ;;
esac

if [[ "${OS}" == "UNKNOWN" ]]; then
  echo "Unsupported OS: $(uname -s)"
  echo "Report: https://github.com/${REPO}/issues"
  exit 1
fi

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

FILE="yutu-${OS}-${ARCH}"
TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT

curl -sSfL "${BASE_URL}/${FILE}" -o "${TMP}/${FILE}"
curl -sSfL "${BASE_URL}/SHA256SUMS" -o "${TMP}/SHA256SUMS"

EXPECTED="$(awk -v f="${FILE}" '$2 == f || $2 == "*" f {print $1}' "${TMP}/SHA256SUMS")"
if [[ -z "${EXPECTED}" ]]; then
  echo "checksum missing: ${FILE} is not listed in SHA256SUMS" >&2
  exit 1
fi
ACTUAL="$(sha256 "${TMP}/${FILE}")"
if [[ "${ACTUAL}" != "${EXPECTED}" ]]; then
  echo "checksum mismatch: ${FILE} has sha256 ${ACTUAL}, SHA256SUMS lists ${EXPECTED}" >&2
  exit 1
fi
echo "checksum verified: ${FILE} sha256 ${ACTUAL}"

if [[ "${YUTU_SKIP_ATTESTATION:-}" == "1" ]]; then
  echo "attestation skipped: YUTU_SKIP_ATTESTATION=1"
elif ! command -v gh >/dev/null 2>&1; then
  echo "attestation skipped: gh not found (install the GitHub CLI to verify build provenance)"
elif ! gh auth status >/dev/null 2>&1; then
  echo "attestation skipped: gh is not signed in (run gh auth login to verify build provenance)"
elif [[ -z "${REPO}" ]]; then
  echo "attestation skipped: YUTU_REPO is not set, so the signing repository is unknown"
elif gh attestation verify "${TMP}/${FILE}" --repo "${REPO}" \
    --signer-workflow "${REPO}/.github/workflows/release.yml" >"${TMP}/attestation.log" 2>&1; then
  echo "attestation verified: ${FILE} was built by ${REPO}/.github/workflows/release.yml"
else
  cat "${TMP}/attestation.log" >&2
  echo "attestation failed: ${FILE} has no valid build-provenance attestation from ${REPO}" >&2
  echo "Set YUTU_SKIP_ATTESTATION=1 to install without it." >&2
  exit 1
fi

mkdir -p "${INSTALL_DIR}"
chmod +x "${TMP}/${FILE}"
mv -f "${TMP}/${FILE}" "${INSTALL_DIR}/yutu"
"${INSTALL_DIR}/yutu" version

if [[ "${WITH_SKILL}" == "1" ]]; then
  if [[ -z "${REPO}" ]]; then
    echo "Skill skipped: set YUTU_REPO to the yutu-rs repository, which holds the skill."
  elif command -v npx >/dev/null 2>&1; then
    echo "Installing skill..."
    if npx --yes skills add "${REPO}" --skill youtube -y; then
      echo "Skill installed."
    else
      echo "Skill install failed. Run manually:"
      echo "  npx skills add ${REPO} --skill youtube"
    fi
  else
    echo "Skill skipped: npx not found."
  fi
fi

cat <<EOF

yutu🐰 installed:
  ${INSTALL_DIR}/yutu

Add ${INSTALL_DIR} to PATH if needed.
EOF
