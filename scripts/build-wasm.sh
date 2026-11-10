#!/bin/sh
# Build src/wasm/zarrita.wasm. CI rebuilds it and fails if the committed file
# differs, so the output must not depend on the machine: path prefixes that
# name a home directory are remapped to fixed ones.
set -eu
cd "$(dirname "$0")/../../.."
sysroot=$(rustc --print sysroot)
commit=$(rustc -vV | sed -n 's/^commit-hash: //p')
cargo_home=${CARGO_HOME:-$HOME/.cargo}
export RUSTFLAGS="-C target-feature=+simd128,+bulk-memory --remap-path-prefix=$sysroot/lib/rustlib/src/rust=/rustc/$commit --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$PWD=/build"
export CFLAGS_wasm32_unknown_unknown="-msimd128 -mbulk-memory -O3 -ffile-prefix-map=$cargo_home=/cargo -ffile-prefix-map=$PWD=/build"
cargo build -p zarrita-rs --target wasm32-unknown-unknown --profile wasm
cp target/wasm32-unknown-unknown/wasm/zarrita_rs.wasm packages/zarrita-rs/src/wasm/zarrita.wasm
node packages/zarrita-rs/scripts/wasm-fingerprint.mjs --write
