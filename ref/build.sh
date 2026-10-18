#!/usr/bin/env bash
# Builds the Go oracle binaries from eat-pray-ai/yutu at the pinned commit.
set -euo pipefail
cd "$(dirname "$0")"
REPO=https://github.com/eat-pray-ai/yutu
REV=bbf38cb8be54d8419cd161d713d8f273ac533a4d
STAMP_VERSION=${STAMP_VERSION:-v0.11.0}
STAMP_DATE=${STAMP_DATE:-2026-10-15T00:00:00Z}
GO=${GO:-$(cd .. && pwd)/.tools/go/bin/go}
export GOPATH=${GOPATH:-$(cd .. && pwd)/.tools/gopath}
export GOTOOLCHAIN=local GOFLAGS=-mod=mod CGO_ENABLED=0

"$GO" version | grep -q 'go1.27.1 ' || { echo "need go1.27.1, got $("$GO" version)"; exit 1; }
if [ ! -d src/.git ]; then git clone --quiet "$REPO" src; fi
git -C src fetch --quiet origin "$REV" 2>/dev/null || true
git -C src checkout --quiet --detach "$REV"
test "$(git -C src rev-parse HEAD)" = "$REV"
(cd src && "$GO" mod verify)

OS=$("$GO" env GOOS) ARCH=$("$GO" env GOARCH)
LD="-s -w -X github.com/eat-pray-ai/yutu/cmd.Version=$STAMP_VERSION -X github.com/eat-pray-ai/yutu/cmd.Commit=$REV -X github.com/eat-pray-ai/yutu/cmd.CommitDate=$STAMP_DATE -X github.com/eat-pray-ai/yutu/cmd.Os=$OS -X github.com/eat-pray-ai/yutu/cmd.Arch=$ARCH -X github.com/eat-pray-ai/yutu/cmd.Builder=Gopher"
(cd src && "$GO" build -trimpath -tags=netgo -ldflags "$LD" -o ../yutu-go .)
(cd harness && "$GO" build -buildvcs=false -trimpath -tags=netgo -ldflags "$LD" -o ../yutu-go-harness .)
(cd harness && "$GO" build -buildvcs=false -trimpath -tags=netgo -ldflags "$LD" -o ../yutu-go-dump ./dump)
shasum -a 256 yutu-go yutu-go-harness yutu-go-dump
