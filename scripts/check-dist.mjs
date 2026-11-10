// Smoke-test the built entries (dist/ and dist/inline/) against the fixtures:
// every array must read the same through both as through the source tests'
// reference, zarrita.js's own codecs (`configureWasm({ enabled: false })`).
// Usage: node scripts/check-dist.mjs   (after `pnpm build`)
import { readdirSync } from 'node:fs'
import FileSystemStore from '@zarrita/storage/fs'

const full = await import('../dist/index.js')
const inline = await import('../dist/inline/index.js')
const fixtures = (v) =>
  new URL(`../fixtures/${v}/data.zarr/`, import.meta.url).pathname

async function read(zarr, [v, name]) {
  try {
    const arr = await zarr.open[v](
      zarr.root(new FileSystemStore(fixtures(v))).resolve(name),
      { kind: 'array' },
    )
    const r = await zarr.get(arr)
    return JSON.stringify([
      r.shape,
      r.stride,
      Array.from(r.data, (v) => String(v)),
    ])
  } catch (e) {
    return `error ${e.name}`
  }
}

const names = ['v2', 'v3'].flatMap((v) =>
  readdirSync(fixtures(v), { withFileTypes: true })
    .filter((d) => d.isDirectory())
    .map((d) => [v, d.name]),
)
const failures = []
full.configureWasm({ enabled: false })
const reference = await Promise.all(names.map((n) => read(full, n)))
full.configureWasm({ enabled: true, gzip: 'wasm' })
inline.configureWasm({ gzip: 'wasm' })
for (const [i, n] of names.entries()) {
  const want = reference[i]
  for (const [label, lib] of [
    ['full', full],
    ['inline', inline],
  ]) {
    const got = await read(lib, n)
    if (got !== want)
      failures.push(
        `${label} ${n.join('/')}: ${got.slice(0, 80)} != ${want.slice(0, 80)}`,
      )
  }
}
full.terminateWasmWorkers()
if (failures.length) {
  console.error(failures.join('\n'))
  process.exit(1)
}
console.log(
  `check-dist: ${names.length} v2/v3 fixtures match zarrita.js in dist/ and dist/inline/`,
)
