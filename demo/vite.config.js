import { defineConfig } from 'vite'

// Cross-origin isolation lets the page also time zarrita-rs's shared-output mode.
const isolation = {
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cross-Origin-Embedder-Policy': 'require-corp',
}

export default defineConfig({
  // The page imports the local build from ../dist.
  server: { port: 8093, headers: isolation, fs: { allow: ['..'] } },
  preview: { headers: isolation },
  worker: { format: 'es' },
  build: { target: 'esnext' },
})
