#!/usr/bin/env bash
# Side by side in Node: native Rust, zarrita.js and zarrita-rs (../dist) on the same data.
# Usage: demo/run.sh [data dir] [iters]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$here/.."
data="${1:-$here/data}"
iters="${2:-10}"
cargo build --release --manifest-path "$root/Cargo.toml" -p zarrita-bench
bench="$root/target/release/zarrita-bench"
[ -d "$data/raw_c64.zarr" ] || "$bench" gen "$data"
"$bench" run "$data" "$iters" > "$here/results-rust.jsonl"
node "$here/bench.mjs" "$data" "$iters" > "$here/results-js.jsonl"
IMPL=zarrita-rs node "$here/bench.mjs" "$data" "$iters" > "$here/results-js-wasm.jsonl"
python3 "$here/compare.py" --impls js,js-wasm,rust,rust-par "$here/results-js.jsonl" "$here/results-js-wasm.jsonl" "$here/results-rust.jsonl"
