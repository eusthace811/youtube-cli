#!/usr/bin/env bash
# Reruns the R11 local lanes and writes docs/evidence/r11/r11-*.txt.
#
# Usage: docs/evidence/r11/lanes.sh [LANE...]   (default: all)
# Environment:
#   YOUTUBE_CLI_BIN   head release binary (default $CARGO_TARGET_DIR/release/youtube)
#   GO_SRC     Go yutu checkout (default ../yutu next to the repository's parent)
#   GO_BIN     stamped Go v0.11.0 build (default ref/yutu-go)
#   GO_ASSET   official Go v0.11.0 darwin-arm64 release asset, if present
#   CDX        cargo-cyclonedx binary directory, for the SBOM probe

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
OUT="${ROOT}/docs/evidence/r11"
TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT}/target}"
BIN="${YOUTUBE_CLI_BIN:-${TARGET_DIR}/release/youtube}"
GO_SRC="${GO_SRC:-/Users/juan/Projects/youtube-cli/yutu}"
GO_BIN="${GO_BIN:-/Users/juan/Projects/youtube-cli/youtube-cli/ref/yutu-go}"
GO_ASSET="${GO_ASSET:-}"
TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) ASSET=youtube-darwin-arm64 NPM_PKG=youtube-darwin-arm64 ;;
  Darwin-x86_64) ASSET=youtube-darwin-amd64 NPM_PKG=youtube-darwin-x64 ;;
  Linux-aarch64) ASSET=youtube-linux-arm64 NPM_PKG=youtube-linux-arm64 ;;
  Linux-x86_64) ASSET=youtube-linux-amd64 NPM_PKG=youtube-linux-x64 ;;
esac

# run CMD...: echo the command, run it, and print its exit code and output.
run() {
  echo "\$ $*"
  "$@" >"${TMP}/stdout" 2>"${TMP}/stderr"
  local code=$?
  echo "exit: ${code}"
  echo "--- stdout ---"
  cat "${TMP}/stdout"
  echo "--- stderr ---"
  cat "${TMP}/stderr"
  echo
  return "${code}"
}

# write N SLUG VERDICT BODY_FILE
write() {
  { echo "r11 lane $1: $3"; echo; cat "$4"; } >"${OUT}/r11-l$1-$2.txt"
  echo "lane $1 ($2): $3"
}

lane1() {
  local body="${TMP}/l1" verdict=PASS
  {
    echo "Generated skills/ against Go's skills/youtube and cmd/agent (INSTRUCTION.md, skills/)."
    echo "Expected differences: E13 (no i18nLanguage-list, i18nRegion-list, videoCategory-list;"
    echo "caption-download under a local-file heading; resources line lists only resources with tools)"
    echo "and the license frontmatter (Go says MIT; every file ships under Apache-2.0); E19: Go's"
    echo "files are first mapped to the new names, and the setup guide and skill metadata point at"
    echo "youtube-cli's install methods instead of Go yutu's."
    echo
    # E19: Go's files under youtube-cli's names first, as parity/diff.py rename_go.
    rm -rf "${TMP}/go-src" && mkdir -p "${TMP}/go-src/cmd/agent"
    cp -R "${GO_SRC}/skills" "${TMP}/go-src/skills"
    cp -R "${GO_SRC}/cmd/agent/skills" "${GO_SRC}/cmd/agent/INSTRUCTION.md" "${TMP}/go-src/cmd/agent/"
    find "${TMP}/go-src" -type f -exec sed -i '' -e 's/YUTU_/YOUTUBE_CLI_/g' -e 's/Yutu/Youtube/g' -e 's/yutu/youtube/g' {} +
    G="${TMP}/go-src"
    run diff -r "${G}/skills/youtube" "${ROOT}/skills/youtube"
    run diff "${G}/cmd/agent/INSTRUCTION.md" "${ROOT}/crates/youtube-agent/assets/INSTRUCTION.md"
    run diff -r "${G}/cmd/agent/skills" "${ROOT}/crates/youtube-agent/assets/skills"
  } >"${body}"
  # Every changed line, deduplicated, must be one of the expected ones.
  diff -r "${G}/skills/youtube" "${ROOT}/skills/youtube" >"${TMP}/d1"
  diff -r "${G}/cmd/agent/skills" "${ROOT}/crates/youtube-agent/assets/skills" >>"${TMP}/d1"
  diff "${G}/cmd/agent/INSTRUCTION.md" "${ROOT}/crates/youtube-agent/assets/INSTRUCTION.md" >>"${TMP}/d1"
  grep -E '^[<>]' "${TMP}/d1" | sort -u >"${TMP}/changed"
  cat >"${TMP}/expected" <<'EOF'
< - `caption-download`
< - `i18nLanguage-list`
< - `i18nRegion-list`
< - `videoCategory-list`
< Manage YouTube resources via MCP tool calls — abuseReport, activity, caption, channel, channelBanner, channelSection, comment, commentThread, i18nLanguage, i18nRegion, liveBroadcast, liveChatBan, liveChatMessage, liveChatModerator, liveStream, member, membershipsLevel, playlist, playlistImage, playlistItem, search, subscription, superChatEvent, thirdPartyLink, thumbnail, video, videoAbuseReportReason, videoCategory, watermark.
< license: MIT
<   author: eat-pray-ai
<   homepage: "https://github.com/eat-pray-ai/youtube"
>   author: youtube-cli
< 
< # Linux / macOS (shell script)
< # Node.js (all platforms)
< # Windows
< # macOS
< ### Other platforms
< Download a prebuilt binary from the [releases page](https://github.com/eat-pray-ai/youtube/releases/latest) and place it in your PATH.
< For more details, see the [README](https://github.com/eat-pray-ai/youtube#readme).
< Install `youtube` using one of these methods:
< brew install youtube
< compatibility: Requires the youtube CLI binary (installable via npm, brew, or winget) and Google Cloud OAuth credentials for YouTube Data API v3.
< curl -sSfL https://raw.githubusercontent.com/eat-pray-ai/youtube/main/scripts/install.sh | bash
< npm i -g @eat-pray-ai/youtube
< npx skills add eat-pray-ai/youtube --skill youtube
< winget install youtube
> # From source
> # Shell script, with checksum and provenance checks (OWNER/REPO: the youtube-cli repository)
> For more details, see the youtube-cli README.
> Install `youtube` (youtube-cli, macOS or Linux) using one of these methods:
> cargo build --release --locked -p youtube
> compatibility: Requires the youtube CLI binary from youtube-cli (installable via npm, its install script, or from source) and Google Cloud OAuth credentials for YouTube Data API v3.
> curl -sSfL https://raw.githubusercontent.com/OWNER/REPO/main/install.sh | YOUTUBE_CLI_REPO=OWNER/REPO bash
> npx skills add OWNER/REPO --skill youtube   # OWNER/REPO: the youtube-cli repository
>
> - `caption-download`
> ### Local file (writes a file on this machine — confirm the path with user first)
> Manage YouTube resources via MCP tool calls — abuseReport, activity, caption, channel, channelBanner, channelSection, comment, commentThread, liveBroadcast, liveChatBan, liveChatMessage, liveChatModerator, liveStream, member, membershipsLevel, playlist, playlistImage, playlistItem, search, subscription, superChatEvent, thirdPartyLink, thumbnail, video, videoAbuseReportReason, watermark.
> license: Apache-2.0
EOF
  # The blank line diff prints after the new heading is "> " with a trailing space.
  grep -v '^>$' "${TMP}/expected" >"${TMP}/e" && printf '> \n' >>"${TMP}/e"
  sort -u "${TMP}/e" -o "${TMP}/expected"
  {
    echo "Distinct changed lines vs the expected set:"
    run diff "${TMP}/expected" "${TMP}/changed"
  } >>"${body}" || verdict=FAIL
  write 1 skills "${verdict}" "${body}"
}

lane2() {
  local body="${TMP}/l2" verdict=PASS
  {
    echo "Generated scripts/command-test.sh against the head release binary."
    echo "The script runs under set -e, so exit 0 means every invocation exited 0."
    echo "invocations: $(grep -c '^"\$YOUTUBE_CLI_PATH"' "${ROOT}/scripts/command-test.sh")"
    echo
    "${ROOT}/scripts/command-test.sh" "${BIN}" >"${TMP}/rust.out" 2>&1
    echo "exit: $?"
    echo
    echo "Same script against Go v0.11.0 (${GO_BIN}), output diffed with head's."
    echo "Expected differences: the 'Using youtube at' path, 'caption download --help' listing"
    echo "--yes, because Rust gates caption download (divergence E1), 'agent --help' naming the"
    echo "claude-code and codex providers (divergence E16), and the version banner (divergence E19)."
    echo "Go's output is first mapped to the new names (E19), as parity/diff.py rename_go does."
    "${ROOT}/scripts/command-test.sh" "${GO_BIN}" >"${TMP}/go.raw" 2>&1
    echo "go exit: $?"
    # E19: Go's output under youtube-cli's names, as parity/diff.py rename_go.
    sed -e 's/YUTU_/YOUTUBE_CLI_/g' -e 's/yutu/youtube/g' -e 's/ (default \[youtube🐰\])//' "${TMP}/go.raw" >"${TMP}/go.out"
    run diff "${TMP}/go.out" "${TMP}/rust.out"
    echo "The subcommand whose help holds line 257 of head's output: $(sed -n '1,257p' "${TMP}/rust.out" |
      grep -E -- '^(=======|-------) ' | tail -2 | tr '\n' ' ')"
    echo "unexpected differences: $(diff "${TMP}/go.out" "${TMP}/rust.out" | grep -E '^[<>]' |
      grep -v -e '^[<>] Using youtube at: ' -e '^> *--yes  *Confirm the operation$' \
        -e '^> # Claude Code or Codex CLI, signed in already$' -e '^> youtube agent --provider \(claude-code\|codex\)$' \
        -e '^[<>]   -p, --provider string  ' |
      grep -v -F -e '_(\_/)_' -e '│   ▶   │' -e '│ • x • │' -e '╰─/───\─╯' |
      grep -v -E -e '^> youtube-cli v' -e '^> CLI, MCP server, and AI agent to automate YouTube workflows$' -e '^> build: ' |
      wc -l | tr -d ' ')"
    echo "--- head output, first 40 lines ---"
    head -40 "${TMP}/rust.out"
  } >"${body}"
  grep -q '^exit: 0$' "${body}" && grep -q '^unexpected differences: 0$' "${body}" || verdict=FAIL
  write 2 smoke "${verdict}" "${body}"
}

lane3() {
  local body="${TMP}/l3" verdict=PASS rel="${TMP}/release"
  mkdir -p "${rel}"
  cp "${BIN}" "${rel}/${ASSET}"
  (cd "${rel}" && shasum -a 256 "${ASSET}" >SHA256SUMS)
  {
    echo "install.sh from a local release dir (YOUTUBE_CLI_RELEASE_URL=file://...) holding the head binary as ${ASSET}."
    echo "SHA256SUMS:"
    cat "${rel}/SHA256SUMS"
    echo
    echo "1. gh is installed and signed in, so install.sh runs gh attestation verify against YOUTUBE_CLI_REPO."
    echo "   The local build has no published attestation, so this must fail closed:"
    run env YOUTUBE_CLI_RELEASE_URL="file://${rel}" YOUTUBE_CLI_REPO=youtube-cli/youtube-cli XDG_BIN_HOME="${TMP}/bin1" "${ROOT}/install.sh"
    echo "2. Without an attestation to check, skipped explicitly:"
    run env YOUTUBE_CLI_RELEASE_URL="file://${rel}" XDG_BIN_HOME="${TMP}/bin2" YOUTUBE_CLI_SKIP_ATTESTATION=1 "${ROOT}/install.sh"
    echo "3. Tampered binary:"
    printf 'tampered' >>"${rel}/${ASSET}"
    run env YOUTUBE_CLI_RELEASE_URL="file://${rel}" XDG_BIN_HOME="${TMP}/bin3" YOUTUBE_CLI_SKIP_ATTESTATION=1 "${ROOT}/install.sh"
    echo "installed by run 2: $(ls -l "${TMP}/bin2/youtube" | awk '{print $1, $5}')"
    echo "installed by run 3: $(ls "${TMP}/bin3/youtube" 2>&1)"
  } >"${body}"
  grep -q '^checksum verified: ' "${body}" &&
    grep -q '^attestation failed: ' "${body}" &&
    grep -q '^checksum mismatch: ' "${body}" || verdict=FAIL
  write 3 install "${verdict}" "${body}"
}

lane4() {
  local body="${TMP}/l4" verdict=PASS bins="${TMP}/npm-bins" tgz="${TMP}/tgz" prefix="${TMP}/prefix"
  mkdir -p "${bins}" "${tgz}" "${prefix}"
  cp "${BIN}" "${bins}/${ASSET}"
  {
    run "${ROOT}/dist/npm/stage.sh" 0.11.0 "${bins}" "${TMP}/stage" "${NPM_PKG}"
    (cd "${tgz}" && run npm pack "${TMP}/stage/youtube" "${TMP}/stage/${NPM_PKG}")
    ls -l "${tgz}"
    echo
    echo "Offline global install from the two tarballs; --foreground-scripts would print any lifecycle script."
    run npm i -g --offline --foreground-scripts --loglevel info --prefix "${prefix}" \
      "${tgz}/youtube-cli-youtube-0.11.0.tgz" "${tgz}/youtube-cli-${NPM_PKG}-0.11.0.tgz"
    echo "scripts field of every installed package.json:"
    for p in "${prefix}"/lib/node_modules/@youtube-cli/*/package.json; do
      echo "  ${p#"${prefix}"/lib/node_modules/}: $(node -e 'console.log(JSON.stringify(require(process.argv[1]).scripts ?? null))' "${p}")"
    done
    echo "hasInstallScript in the lockfile view: $(npm ls -g --prefix "${prefix}" --json --long 2>/dev/null | grep -c '"hasInstallScript": true')"
    echo
    run "${prefix}/bin/youtube" version
    echo "Exit-code propagation, wrapper vs the binary, for an unknown command:"
    "${prefix}/bin/youtube" nosuchcommand >/dev/null 2>&1
    echo "  wrapper exit: $?"
    "${BIN}" nosuchcommand >/dev/null 2>&1
    echo "  binary exit:  $?"
  } >"${body}"
  grep -q 'youtube-cli v0.11.0' "${body}" && ! grep -qiE 'postinstall|preinstall|"install"' "${body}" || verdict=FAIL
  write 4 npm "${verdict}" "${body}"
}

lane5() {
  local body="${TMP}/l5"
  {
    echo "SUBSTITUTED: no Docker on this machine. The image cannot be built or run here."
    echo "Shown instead: the Dockerfile, and the documented invocation run on the head binary."
    echo
    echo "--- dist/Dockerfile ---"
    cat "${ROOT}/dist/Dockerfile"
    echo
    echo "The documented container command is: youtube mcp --mode http --host 0.0.0.0 --port 8216"
    echo "\$ ${BIN} mcp --mode http --host 0.0.0.0 --port 18216"
    "${BIN}" mcp --mode http --host 0.0.0.0 --port 18216 >"${TMP}/l5.out" 2>&1 &
    local pid=$!
    sleep 2
    echo "listeners on 18216 after 2s: $(lsof -nP -iTCP:18216 -sTCP:LISTEN 2>/dev/null | tail -n +2)"
    kill "${pid}" 2>/dev/null
    wait "${pid}"
    echo "exit: $?"
    cat "${TMP}/l5.out"
    echo
    echo "An unknown flag fails at parse time, so --host and --port above were parsed:"
    run "${BIN}" mcp --mode http --nosuchflag 1
  } >"${body}" 2>&1
  write 5 docker SUBSTITUTED "${body}"
}

lane6() {
  local body="${TMP}/l6"
  {
    echo "SUBSTITUTED: attesting needs a tag push on a GitHub repository; none exists for youtube-cli yet."
    echo "Evidence will come from the first tag push: release.yml's release job runs"
    echo "actions/attest-build-provenance on the six binaries, then"
    echo "  gh attestation verify youtube-<os>-<arch> --repo <repo> --signer-workflow <repo>/.github/workflows/release.yml"
    echo "which is the command install.sh runs, must print verified."
    echo
    echo "--- release.yml attest and publish steps ---"
    sed -n '/attest-build-provenance/,$p' "${ROOT}/.github/workflows/release.yml"
    echo
    if [[ -n "${GO_ASSET}" && -f "${GO_ASSET}" ]]; then
      echo "The same gh command against a real attested release, Go v0.11.0's darwin-arm64 asset:"
      run gh attestation verify "${GO_ASSET}" --repo eat-pray-ai/yutu
      echo "Its verified attestations (signer workflow, predicate type):"
      gh attestation verify "${GO_ASSET}" --repo eat-pray-ai/yutu --format json |
        node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{for(const a of JSON.parse(s))console.log("  ",a.verificationResult.signature.certificate.buildSignerURI,a.verificationResult.statement.predicateType)})'
    fi
    if [[ -n "${CDX:-}" ]]; then
      echo "The SBOM step, run locally (cargo-cyclonedx 0.5.9):"
      PATH="${CDX}:${PATH}" run cargo cyclonedx --manifest-path "${ROOT}/crates/youtube/Cargo.toml" \
        --format json --spec-version 1.5 --target all --no-build-deps --override-filename youtube.cdx
      if [[ -f "${ROOT}/crates/youtube/youtube.cdx.json" ]]; then
        node -e 'const b=require(process.argv[1]);console.log("bomFormat", b.bomFormat, "specVersion", b.specVersion, "components", b.components.length, "top", b.metadata.component.name)' "${ROOT}/crates/youtube/youtube.cdx.json"
      fi
      # cargo-cyclonedx writes one SBOM per workspace member; release.yml keeps crates/youtube's.
      rm -f "${ROOT}"/crates/*/youtube.cdx.json "${ROOT}/xtask/youtube.cdx.json"
    fi
  } >"${body}" 2>&1
  write 6 attest SUBSTITUTED "${body}"
}

lane7() {
  local body="${TMP}/l7" verdict=PASS
  {
    echo "The zsh script from 'youtube completion zsh' asks the binary through __complete,"
    echo "so these two calls are what zsh runs for 'youtube vid<TAB>' and 'youtube video l<TAB>':"
    run "${BIN}" __complete vid
    run "${BIN}" __complete video l
    echo "The generated zsh script calls __complete:"
    "${BIN}" completion zsh | grep -n '__complete' | head -5
    echo
    echo "zsh, non-interactive: source the script and check the completion function is defined:"
    run zsh -c "autoload -U compinit && compinit -u -d ${TMP}/zcompdump; source <(\"${BIN}\" completion zsh); whence -w _youtube"
  } >"${body}" 2>&1
  grep -q '^video	' "${body}" && grep -q '^list	' "${body}" || verdict=FAIL
  write 7 completion "${verdict}" "${body}"
}

lane8() {
  local body="${TMP}/l8" verdict=PASS schema="${TMP}/schema/server.schema.json" url
  url="https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json"
  mkdir -p "${TMP}/schema" "${TMP}/scripts"
  cat >"${TMP}/scripts/validate.py" <<'EOF'
import json, sys
import jsonschema
schema = json.load(open(sys.argv[1]))
for path in sys.argv[2:]:
    doc = json.load(open(path))
    errors = list(jsonschema.Draft7Validator(schema).iter_errors(doc))
    print(f"{path}: {len(errors)} errors")
    for e in errors:
        print("  ", list(e.path), e.message)
    if errors:
        sys.exit(1)
EOF
  {
    run curl -sSfL "${url}" -o "${schema}"
    echo "schema sha256: $(shasum -a 256 "${schema}" | awk '{print $1}')"
    python3 -m pip install --quiet --target "${TMP}/pylib" jsonschema >/dev/null 2>&1
    echo "Validating dist/server.json, and Go's server.json as a control:"
    run env PYTHONPATH="${TMP}/pylib" python3 "${TMP}/scripts/validate.py" \
      "${schema}" "${ROOT}/dist/server.json" "${GO_SRC}/server.json"
    echo "mcpName of the wrapper: $(node -e 'console.log(require(process.argv[1]).mcpName)' "${ROOT}/dist/npm/youtube/package.json")"
    echo "name in server.json:    $(node -e 'console.log(require(process.argv[1]).name)' "${ROOT}/dist/server.json")"
  } >"${body}" 2>&1
  grep -q 'dist/server.json: 0 errors' "${body}" || verdict=FAIL
  write 8 registry "${verdict}" "${body}"
}

lane9() {
  local body="${TMP}/l9"
  {
    echo "DEFERRED: Windows is not built in this release (decision R12, 2026-10-18). The file sandbox's"
    echo "identity checks, the console approval's terminal handling and the CLI-provider process groups"
    echo "use Unix APIs with no Windows port yet, and no Windows host was available to build or test one."
    echo "ci.yml and release.yml build macOS and Linux only; npm ships no win32 packages; no winget manifest."
    echo
    echo "The help-parity diff the Windows job would have run, on this host:"
    "${GO_BIN}" video list --help >"${TMP}/go-help" 2>&1
    "${BIN}" video list --help >"${TMP}/rs-help" 2>&1
    run diff -u "${TMP}/go-help" "${TMP}/rs-help"
  } >"${body}" 2>&1
  write 9 windows DEFERRED "${body}"
}

lane10() {
  local body="${TMP}/l10" verdict=PASS
  {
    echo "Host release binary only; release.yml runs the same check on all six before publishing."
    echo "At b82bce4 this failed on three display-only error-message URLs (two rustls docs.rs"
    echo "links, the youtube-auth README#prerequisites link). R11 lists them exactly in DISPLAY_ONLY."
    (cd "${ROOT}" && run cargo run -q --locked -p xtask -- egress "${BIN}")
  } >"${body}" 2>&1
  grep -q 'egress: 0 unexpected hosts' "${body}" || verdict=FAIL
  write 10 egress "${verdict}" "${body}"
}

perf() {
  local body="${TMP}/perf" verdict=PASS
  local rs go go_asset rs_tgz go_tgz certs
  rs=$(wc -c <"${BIN}" | tr -d ' ')
  go=$(wc -c <"${GO_BIN}" | tr -d ' ')
  mkdir -p "${TMP}/pt/bins" "${TMP}/pt/tgz"
  cp "${BIN}" "${TMP}/pt/bins/${ASSET}"
  "${ROOT}/dist/npm/stage.sh" 0.11.0 "${TMP}/pt/bins" "${TMP}/pt/stage" "${NPM_PKG}" >/dev/null
  (cd "${TMP}/pt/tgz" && npm pack --silent "${TMP}/pt/stage/${NPM_PKG}" >/dev/null 2>&1)
  rs_tgz=$(wc -c <"${TMP}/pt/tgz/youtube-cli-${NPM_PKG}-0.11.0.tgz" | tr -d ' ')
  local go_pkg="${NPM_PKG/youtube/yutu}"
  go_tgz=$(curl -sSfL "https://registry.npmjs.org/@eat-pray-ai/${go_pkg}/-/${go_pkg}-0.11.0.tgz" | wc -c | tr -d ' ')
  certs=$(curl -sSfL https://curl.se/ca/cacert.pem | wc -c | tr -d ' ')
  ratio() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.3f", a / b }'; }
  check() { awk -v r="$1" 'BEGIN { exit !(r <= 0.3) }' || verdict=FAIL; }
  {
    echo "Rule: head <= 0.3x Go on every channel. Sizes in bytes."
    echo
    echo "raw binary (${ASSET})"
    echo "  Go v0.11.0 stamped build ${GO_BIN}: ${go}"
    if [[ -n "${GO_ASSET}" && -f "${GO_ASSET}" ]]; then
      go_asset=$(wc -c <"${GO_ASSET}" | tr -d ' ')
      echo "  Go v0.11.0 official release asset:    ${go_asset}"
    fi
    echo "  head ${BIN}: ${rs}"
    echo "  ratio vs stamped build: $(ratio "${rs}" "${go}")"
    [[ -n "${go_asset:-}" ]] && echo "  ratio vs official asset: $(ratio "${rs}" "${go_asset}")"
    echo
    echo "npm platform tarball (@*/${NPM_PKG}@0.11.0)"
    echo "  Go @eat-pray-ai tarball from registry.npmjs.org: ${go_tgz}"
    echo "  head @youtube-cli tarball from npm pack: ${rs_tgz}"
    echo "  ratio: $(ratio "${rs_tgz}" "${go_tgz}")"
    echo
    echo "Docker image (SUBSTITUTED: no Docker here; estimated as uncompressed layer bytes)"
    echo "  Go image = distroless/static-debian13 base (about 2,000,000) + Go linux binary"
    echo "  head image = scratch + CA bundle (Mozilla bundle, ${certs}; Alpine's is the same set) + head linux musl binary"
    echo "  Using the host binaries as stand-ins for the linux ones:"
    echo "  Go estimate:   $((2000000 + go))"
    echo "  head estimate: $((certs + rs))"
    echo "  ratio: $(ratio "$((certs + rs))" "$((2000000 + go))")"
  } >"${body}"
  check "$(ratio "${rs}" "${go}")"
  check "$(ratio "${rs_tgz}" "${go_tgz}")"
  check "$(ratio "$((certs + rs))" "$((2000000 + go))")"
  { echo "r11 perf: ${verdict}"; echo; cat "${body}"; } >"${OUT}/r11-perf.txt"
  echo "perf: ${verdict}"
}

lanes=("$@")
[[ ${#lanes[@]} -eq 0 ]] && lanes=(1 2 3 4 5 6 7 8 9 10 perf)
for l in "${lanes[@]}"; do
  case "${l}" in
    perf) perf ;;
    *) "lane${l}" ;;
  esac
done
