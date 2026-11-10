// Fingerprint of the Rust sources behind src/wasm/zarrita.wasm, stored in
// src/wasm/zarrita.wasm.inputs. The wasm is committed and is not
// byte-reproducible across build hosts, so CI cannot rebuild and diff it;
// instead `--check` fails when the sources or the wasm changed without a
// rebuild. build-wasm.sh runs `--write` after each build.
// usage: node scripts/wasm-fingerprint.mjs --write | --check
import { createHash } from 'node:crypto'
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, relative, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const pkg = join(dirname(fileURLToPath(import.meta.url)), '..')
const root = join(pkg, '..', '..')
const wasm = join(pkg, 'src/wasm/zarrita.wasm')
const stampPath = `${wasm}.inputs`

const inputs = [
  join(root, 'Cargo.toml'),
  join(root, 'Cargo.lock'),
  join(root, 'rust-toolchain.toml'),
  join(pkg, 'scripts/build-wasm.sh'),
  ...readdirSync(join(pkg, 'crates'), { recursive: true, withFileTypes: true })
    .filter((e) => e.isFile())
    .map((e) => join(e.parentPath, e.name)),
]
  .map((p) => relative(root, p).split(sep).join('/'))
  .sort()

const sources = createHash('sha256')
for (const p of inputs) {
  sources.update(`${p}\0`)
  // Line endings normalized so a Windows checkout gets the same fingerprint.
  sources.update(readFileSync(join(root, p), 'utf8').replace(/\r\n/g, '\n'))
  sources.update('\0')
}
const stamp = `sources ${sources.digest('hex')}\nwasm ${createHash('sha256').update(readFileSync(wasm)).digest('hex')}\n`

const mode = process.argv[2]
if (mode === '--write') {
  writeFileSync(stampPath, stamp)
} else if (mode === '--check') {
  let committed = ''
  try {
    committed = readFileSync(stampPath, 'utf8')
  } catch {}
  if (committed !== stamp) {
    console.error(
      'zarrita-rs: src/wasm/zarrita.wasm is out of date with its Rust sources.\n' +
        'Rebuild it with `bunx nx build:wasm zarrita-rs` and commit zarrita.wasm and zarrita.wasm.inputs.',
    )
    process.exit(1)
  }
} else {
  console.error('usage: wasm-fingerprint.mjs --write | --check')
  process.exit(2)
}
