# syntax=docker/dockerfile:1
# Copyright 2025 eat-pray-ai & OpenWaygate
# SPDX-License-Identifier: Apache-2.0
#
# The youtube image: the static Linux release binary, the CA roots it needs to
# reach Google over TLS, and nothing else. It runs as a non-root user.
#
# Build from a directory holding the release assets youtube-linux-amd64 and
# youtube-linux-arm64 (static musl builds from release.yml):
#
#   docker buildx build --platform linux/amd64,linux/arm64 \
#     -f dist/Dockerfile -t youtube-cli <release-dir>
#
# Run a command, mounting the directory that holds client_secret.json and
# youtube.token.json as the working directory /app:
#
#   docker run --rm -v "$PWD:/app" youtube-cli channel list --for mine
#
# MCP over HTTP (divergence E14). youtube binds 127.0.0.1 unless told
# otherwise, and the container's loopback is not reachable from the host, so
# pass --host 0.0.0.0 inside the container and publish the port on the host's
# loopback only:
#
#   docker run --rm -p 127.0.0.1:8216:8216 -v "$PWD:/app" \
#     youtube-cli mcp --mode http --host 0.0.0.0 --port 8216
#
# The server is then at http://127.0.0.1:8216/mcp on the host. Inside the
# container 0.0.0.0 is reachable by anything that can reach the container's
# network, so keep the -p 127.0.0.1: prefix, and put a TLS proxy in front
# before exposing it beyond this machine. See dist/README.md.

FROM docker.io/library/alpine:3.22@sha256:5291449c3df73caf6ed85e649dec1b9e818b39a5d8c871e97afc13e9cd5e8fa8 AS stage
ARG TARGETARCH
RUN apk add --no-cache ca-certificates
COPY youtube-linux-${TARGETARCH} /youtube
RUN chmod 0755 /youtube && mkdir -m 0700 /app && chown 65532:65532 /app

FROM scratch
COPY --from=stage /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=stage /youtube /youtube
COPY --from=stage --chown=65532:65532 /app /app
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt
USER 65532:65532
WORKDIR /app
EXPOSE 8216/tcp
LABEL io.modelcontextprotocol.server.name="io.github.eat-pray-ai/yutu-rs"
ENTRYPOINT ["/youtube"]
CMD ["--help"]
