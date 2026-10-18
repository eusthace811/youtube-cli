# syntax=docker/dockerfile:1
# Copyright 2025 eat-pray-ai & OpenWaygate
# SPDX-License-Identifier: Apache-2.0
#
# The yutu image: the static Linux release binary, the CA roots it needs to
# reach Google over TLS, and nothing else. It runs as a non-root user.
#
# Build from a directory holding the release assets yutu-linux-amd64 and
# yutu-linux-arm64 (static musl builds from release.yml):
#
#   docker buildx build --platform linux/amd64,linux/arm64 \
#     -f dist/Dockerfile -t yutu-rs <release-dir>
#
# Run a command, mounting the directory that holds client_secret.json and
# youtube.token.json as the working directory /app:
#
#   docker run --rm -v "$PWD:/app" yutu-rs channel list --for mine
#
# MCP over HTTP (divergence E14). yutu binds 127.0.0.1 unless told
# otherwise, and the container's loopback is not reachable from the host, so
# pass --host 0.0.0.0 inside the container and publish the port on the host's
# loopback only:
#
#   docker run --rm -p 127.0.0.1:8216:8216 -v "$PWD:/app" \
#     yutu-rs mcp --mode http --host 0.0.0.0 --port 8216
#
# The server is then at http://127.0.0.1:8216/mcp on the host. Inside the
# container 0.0.0.0 is reachable by anything that can reach the container's
# network, so keep the -p 127.0.0.1: prefix, and put a TLS proxy in front
# before exposing it beyond this machine. See dist/README.md.

FROM docker.io/library/alpine:3.22@sha256:5291449c3df73caf6ed85e649dec1b9e818b39a5d8c871e97afc13e9cd5e8fa8 AS stage
ARG TARGETARCH
RUN apk add --no-cache ca-certificates
COPY yutu-linux-${TARGETARCH} /yutu
RUN chmod 0755 /yutu && mkdir -m 0700 /app && chown 65532:65532 /app

FROM scratch
COPY --from=stage /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=stage /yutu /yutu
COPY --from=stage --chown=65532:65532 /app /app
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt
USER 65532:65532
WORKDIR /app
EXPOSE 8216/tcp
LABEL io.modelcontextprotocol.server.name="io.github.eat-pray-ai/yutu-rs"
ENTRYPOINT ["/yutu"]
CMD ["--help"]
