# Distribution

Every channel ships the same release binaries that `.github/workflows/release.yml` builds on a tag push. The assets keep Go yutu's names, `yutu-{darwin,linux,windows}-{amd64,arm64}`, with `.exe` on Windows. Each release also carries `SHA256SUMS`, a CycloneDX SBOM (`yutu.cdx.json`), and a GitHub build-provenance attestation per binary. Linux binaries are static musl builds, so they run on any distribution and in a `scratch` image.

Nothing in this directory publishes anything. Publishing to npm, a container registry, winget, or the MCP registry is a separate, operator-run step.

## Shell installer

`install.sh` at the repository root downloads the binary for the platform, checks it against the release's `SHA256SUMS`, and stops with `checksum mismatch` and exit 1 when they differ. When `gh` is installed it also runs `gh attestation verify --repo <repo> --signer-workflow <repo>/.github/workflows/release.yml` and stops when that fails. It installs the agent skill only with `--with-skill`.

```shell
curl -sSfL https://raw.githubusercontent.com/eat-pray-ai/yutu/main/install.sh | bash
curl -sSfL https://raw.githubusercontent.com/eat-pray-ai/yutu/main/install.sh | bash -s -- --with-skill
```

`YUTU_REPO` picks the GitHub repository, `YUTU_RELEASE_URL` replaces the download base URL (any URL curl reads, including `file://`), and `XDG_BIN_HOME` picks the install directory.

## npm

`npm/` holds the wrapper package `@yutu-rs/yutu` and six platform packages `@yutu-rs/yutu-{darwin,linux,win32}-{arm64,x64}`, with Go's layout. The wrapper lists the platform packages as optional dependencies, so npm installs only the one whose `os` and `cpu` match. Its `bin/yutu` resolves that package and runs the binary with inherited stdio, propagating the exit code. No package has an install or postinstall script.

The scope is new because `@eat-pray-ai` is the Go package's scope, and publishing the Rust build there would replace Go's binaries for every existing user. `@yutu-rs` keeps the two installable side by side until cutover. The scope must be created on npm by the operator before the first publish.

`npm/stage.sh VERSION BIN_DIR OUT_DIR [PACKAGE...]` copies the packages to `OUT_DIR`, stamps the version, and adds the binaries and `LICENSE`, leaving the sources untouched. `npm pack` or `npm publish` then runs in each staged directory.

## Docker

`Dockerfile` builds a `scratch` image holding the static binary and the Alpine CA bundle, running as UID 65532 in `/app`. Build it from a directory with `yutu-linux-amd64` and `yutu-linux-arm64`:

```shell
docker buildx build --platform linux/amd64,linux/arm64 -f dist/Dockerfile -t yutu-rs <release-dir>
docker run --rm -v "$PWD:/app" yutu-rs channel list --for mine
```

### MCP over HTTP from a container (divergence E14)

`yutu mcp --mode http` binds `127.0.0.1` by default. Inside a container that is the container's own loopback, which the host cannot reach through `-p`. Go's image has the same default and so cannot serve HTTP MCP to the host at all. Bind every interface inside the container, and publish the port on the host's loopback only:

```shell
docker run --rm -p 127.0.0.1:8216:8216 -v "$PWD:/app" \
  yutu-rs mcp --mode http --host 0.0.0.0 --port 8216
```

The server is then at `http://127.0.0.1:8216/mcp`. Binding `0.0.0.0` makes the server reachable by anything on the container's network, such as other containers on the same bridge, and plain `-p 8216:8216` would expose it on every host interface. Keep the `127.0.0.1:` prefix. To serve other machines, terminate TLS in a reverse proxy in front of it. HTTP mode (R8) still requires a Google bearer token on every request and rejects non-loopback `Origin` headers.

## MCP registry

`server.json` is the registry entry for `io.github.eat-pray-ai/yutu-rs`, on schema 2025-12-11, mirroring Go's entry with a stdio and a streamable-HTTP package, both `@yutu-rs/yutu`. The wrapper's `mcpName` matches it. The release pipeline replaces `0.0.0-dev` with the tag version before `mcp-publisher publish`. `YUTU_LOG_LEVEL` lists `WARN`, the spelling yutu reads, where Go's entry listed `WARNING`, which yutu treats as `INFO`.

## winget

`winget/` holds the three manifest templates for `eat-pray-ai.yutu-rs`, a portable `.exe` per architecture. `winget/render.sh VERSION SHA256SUMS OUT_DIR [REPO]` fills in the version and the installer hashes in the `microsoft/winget-pkgs` layout. The release workflow renders them as the `winget-manifests` artifact, ready to submit.
