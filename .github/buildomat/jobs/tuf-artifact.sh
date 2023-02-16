#!/bin/bash
#:
#: name = "helios / tuf-artifact"
#: variety = "basic"
#: target = "helios-latest"
#: rust_toolchain = "nightly-2022-09-27"
#: output_rules = [
#:	"=/work/repo.zip",
#: ]
#:
#: [dependencies.package]
#: job = "helios / package"
#:

set -o errexit
set -o pipefail
set -o xtrace

cargo --version
rustc --version

COMMIT=$(git rev-parse HEAD)
VERSION="0.0.0+git${COMMIT:0:11}"

pushd /input/package/work/out
tar cvf "/work/control-plane.tar" ./*
popd

cargo build --locked --release --bin tufaceous
alias tufaceous=target/release/tufaceous

python3 -c 'import secrets; open("/work/key.txt", "w").write("ed25519:%s\n" % secrets.token_hex(32))'
read -r TUFACEOUS_KEY </work/key.txt
export TUFACEOUS_KEY

tufaceous -r /work/repo init --no-generate-key
tufaceous -r /work/repo add --name control_plane control_plane "/work/control-plane.tar" "$VERSION"
tufaceous -r /work/repo archive /work/repo.zip
