#!/usr/bin/env bash
# Copyright 2025 eat-pray-ai & OpenWaygate
# SPDX-License-Identifier: Apache-2.0
#
# Installs the youtube release binary for this platform into
# ${XDG_BIN_HOME:-$HOME/.local/bin}, after checking its SHA-256 against the
# release's SHA256SUMS and, when `gh` is available, its build-provenance
# attestation.
#
# Usage: install.sh [--with-skill]
#   --with-skill   also install the youtube agent skill with `npx skills add`
#
# Environment:
#   YOUTUBE_CLI_REPO               GitHub repository of the youtube-cli releases, owner/name
#                           (required unless YOUTUBE_CLI_RELEASE_URL is set; the release
#                           notes give the command with it filled in)
#   YOUTUBE_CLI_RELEASE_URL        base URL of the release assets, any URL curl reads
#                           (default https://github.com/$YOUTUBE_CLI_REPO/releases/latest/download)
#   YOUTUBE_CLI_SKIP_ATTESTATION=1 skip the attestation check even when gh is present

set -euo pipefail

REPO="${YOUTUBE_CLI_REPO:-}"
BASE_URL="${YOUTUBE_CLI_RELEASE_URL:-https://github.com/${REPO}/releases/latest/download}"
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

if [[ -z "${REPO}" && -z "${YOUTUBE_CLI_RELEASE_URL:-}" ]]; then
  echo "Set YOUTUBE_CLI_REPO to the GitHub repository of the youtube-cli releases (owner/name)." >&2
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

FILE="youtube-${OS}-${ARCH}"
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

if [[ "${YOUTUBE_CLI_SKIP_ATTESTATION:-}" == "1" ]]; then
  echo "attestation skipped: YOUTUBE_CLI_SKIP_ATTESTATION=1"
elif ! command -v gh >/dev/null 2>&1; then
  echo "attestation skipped: gh not found (install the GitHub CLI to verify build provenance)"
elif ! gh auth status >/dev/null 2>&1; then
  echo "attestation skipped: gh is not signed in (run gh auth login to verify build provenance)"
elif [[ -z "${REPO}" ]]; then
  echo "attestation skipped: YOUTUBE_CLI_REPO is not set, so the signing repository is unknown"
elif gh attestation verify "${TMP}/${FILE}" --repo "${REPO}" \
    --signer-workflow "${REPO}/.github/workflows/release.yml" >"${TMP}/attestation.log" 2>&1; then
  echo "attestation verified: ${FILE} was built by ${REPO}/.github/workflows/release.yml"
else
  cat "${TMP}/attestation.log" >&2
  echo "attestation failed: ${FILE} has no valid build-provenance attestation from ${REPO}" >&2
  echo "Set YOUTUBE_CLI_SKIP_ATTESTATION=1 to install without it." >&2
  exit 1
fi

mkdir -p "${INSTALL_DIR}"
chmod +x "${TMP}/${FILE}"
mv -f "${TMP}/${FILE}" "${INSTALL_DIR}/youtube"
"${INSTALL_DIR}/youtube" version

if [[ "${WITH_SKILL}" == "1" ]]; then
  if [[ -z "${REPO}" ]]; then
    echo "Skill skipped: set YOUTUBE_CLI_REPO to the youtube-cli repository, which holds the skill."
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

youtube-cli installed:
  ${INSTALL_DIR}/youtube

Add ${INSTALL_DIR} to PATH if needed.
EOF
