# zarrita-rs

A drop-in replacement for [zarrita.js](https://github.com/manzt/zarrita.js) that
decodes chunks with Rust compiled to WebAssembly, in a pool of Web Workers
(browsers) or `worker_threads` (Node).

```diff
- import * as zarr from "zarrita";
+ import * as zarr from "@niivue/zarrita-rs";
```

Or keep your imports and alias the package: `"zarrita": "npm:@niivue/zarrita-rs@^0.1.0"`.

## How it works

The public API, types, stores and store extensions are zarrita.js 0.7.5's own
code. Only two internals change:

1. **Chunk decoding.** When an array's codec chain contains a compressor the
   wasm core supports (gzip, zlib, zstd or blosc, optionally with
   bytes, transpose, shuffle, delta and crc32c), `decode` runs in wasm (for
   gzip/zlib-only arrays, see the `gzip` option below). This includes the
   inner chunks of sharded arrays. Chunks larger
   than 64 KiB decoded go to a worker; smaller ones decode on the calling
   thread. Everything else (shard indexes, strings, bitround, uncompressed arrays,
   writes) uses zarrita.js's code unchanged.
2. **Copying chunks into the output** uses a faster typed-array kernel.

Codecs you replace via `zarr.registry.set(...)` are always respected: the wasm
path only runs for codecs whose registry entry is still the built-in one. No
`SharedArrayBuffer` or cross-origin isolation is needed.

### Shared output (optional, faster)

When the output is backed by a `SharedArrayBuffer`, workers decode and write
their part of the result directly into it, so the main thread does no
decoding or copying at all. This happens when:

- you pass zarrita.js's existing option `zarr.get(arr, sel, { useSharedArrayBuffer: true })`, or
- you call `configureWasm({ sharedOutput: true })` and the page is
  cross-origin isolated (COOP `same-origin` plus COEP `require-corp` or
  `credentialless`). In Node it always applies.

Results then have `data.buffer instanceof SharedArrayBuffer`. Typed arrays
over shared memory work with WebGL/WebGPU uploads and most APIs, but a few
(for example `TextDecoder.decode`) reject them, which is why it is opt-in.

## Download size

Measured with esbuild code splitting (as Vite/webpack apps get), gzip -9:

| What a page downloads | zarrita.js | zarrita-rs |
|---|---:|---:|
| Library core | 14.5 KB | 18 KB |
| Reading blosc data | + 204 KB (numcodecs blosc) | + 166 KB (`zarrita.wasm`, all codecs) |
| Reading zstd data | + 241 KB (numcodecs zstd) | + 166 KB |
| Reading standalone-lz4 data (legacy) | + 16 KB | + 166 KB (`zarrita.wasm`, shared) |
| Reading gzip/zlib data | + 0 (browser built-in) | + 0 by default (see `gzip` option) |
| Uncompressed data | + 0 | + 0 (wasm is not loaded) |

`zarrita.wasm` is 405 KB raw, 166 KB gzip, 137 KB brotli, and is fetched once
and shared by all workers. It contains decoders only. Of that, zstd is about
41 KB gzip; JSON parsing of codec configs and zlib-rs make up most of the rest.
By default (`gzip: "auto"`), arrays compressed only with gzip or zlib use the
browser's built-in decoder, as zarrita.js does, unless `zarrita.wasm` is
already loaded because another array needed it; then they decode in wasm
workers for free. `gzip: "wasm"` always uses wasm (about 1.8x faster decoding
for a 166 KB download); `gzip: "native"` never does.
zarrita.js's codec modules stay in the package as fallbacks and are only
fetched if the JS path is used.

## Extra API

```ts
zarr.configureWasm({
  enabled: true,        // false = behave exactly like zarrita.js
  sharedOutput: false,  // see "Shared output" above
  gzip: "auto",         // "auto" | "wasm" | "native", see "Download size" above
  workers: 4,           // default: hardware threads, max 8; 0 = no workers
  minWorkerBytes: 65536,
  wasmUrl, workerUrl,   // override asset locations (e.g. a CDN)
});
zarr.terminateWasmWorkers();
```

## `@niivue/zarrita-rs/inline`

A second entry for hosts that already decode in their own workers, or that
bundle into one file or an inline worker (NiiVue's OME-Zarr chunk workers are
both):

```ts
import * as zarr from "@niivue/zarrita-rs/inline";
```

Same API, but the wasm module is embedded in the JavaScript as base64 (in a
lazily imported chunk, about 220 KB gzip), there is no worker pool, the code
never refers to its own URL, and there is no numcodecs fallback. blosc and zstd
stages that cannot take the whole-pipeline fast path (compressed string arrays,
sharding indexes) still decode in wasm; writing blosc/zstd/lz4 is not
available. `bun run test` checks both built entries against zarrita.js.

## Install

Release candidates are published under the `next` dist-tag:

```sh
npm install @niivue/zarrita-rs@next
```

## Bundlers

The package loads `wasm/worker.js` and `wasm/zarrita.wasm` with
`new URL(..., import.meta.url)`. Vite production builds pick them up as
assets automatically (checked with Vite 6). webpack 5 and Parcel support the
same pattern but have not been tested yet.

Vite's **dev server** pre-bundles dependencies, which breaks those URLs. Exclude
the package (checked with Vite 6):

```js
// vite.config.js
export default { optimizeDeps: { exclude: ["@niivue/zarrita-rs"] } };
```

Without it you get an error that says so. Other setups can pass `workerUrl`
and `wasmUrl` to `configureWasm`.

## Building

```sh
bun install
bun run build:wasm  # needs the wasm32-unknown-unknown Rust target and clang (for zstd)
bun run build       # packages the committed src/wasm/zarrita.wasm
bun run test        # zarrita.js's own test suite, wasm-path tests, built-entry check
bun run lint        # Biome and the wasm fingerprint
bun run typecheck
bun run test:rust
bun run lint:rust
```

`src/wasm/zarrita.wasm` is committed, so the JavaScript build needs no Rust
toolchain. `build:wasm` also writes `src/wasm/zarrita.wasm.inputs`, a fingerprint
of the Rust sources and the wasm; commit both files together. `bun run lint`
fails when the Rust sources changed without a rebuild. The wasm is not
byte-identical across build hosts, so CI (`.github/workflows/rust.yml`) builds it
from source and runs the tests against that build rather than diffing it.

## Status

- All 764 zarrita.js 0.7.5 tests pass, plus 10 wasm-path tests (774 total).
- Every fixture array, plus about 300 random slice/step/integer selections,
  reads identically to zarrita.js in all modes (main thread, workers, shared
  output on main thread and in workers).
- The standalone numcodecs `lz4` codec decodes in wasm (lz4_flex), as does
  blosc's internal lz4.
- Sharded arrays: inner chunks decode in wasm and, with shared output, are
  written straight into the result. On a 256^3 int16 volume in 128^3 shards
  of 32^3 blosc-zstd-bitshuffle chunks (Node, in memory, 2 cores), a full
  read takes 96 ms (84 ms shared) vs 177 ms in zarrita.js; a 160^3 region 33
  vs 63 ms; single planes 7-9 vs 10-16 ms.
- Corrupt chunks (truncated, bad checksum) raise a `CodecPipelineError` whose
  cause says `corrupt chunk data`; only codec options the core lacks (such as
  blosc+snappy) fall back to the JS codecs.

## Trying it on your own data

The `zarrita-compare` app in [niivue/mono](https://github.com/niivue/mono)
(`apps/zarrita-compare`) has a page that reads any CORS-enabled Zarr URL with
zarrita.js, zarrita-rs and zarrita-rs with shared output, plus a Node
benchmark. The native Rust benchmark lives here in `bench/`:

```sh
cargo run --release -p zarrita-bench -- gen <dir>      # write the test stores
cargo run --release -p zarrita-bench -- run <dir> [iters]
```
