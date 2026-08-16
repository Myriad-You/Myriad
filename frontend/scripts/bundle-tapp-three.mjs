#!/usr/bin/env node
/**
 * Pin Three r170 + GLTFLoader as a sandbox IIFE.
 * Output is fetched by the host and nonce-inlined; it is not part of the app bundle.
 */
import { mkdir, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { build } from 'esbuild'

const here = dirname(fileURLToPath(import.meta.url))
const entry = join(here, 'tapp-three-entry.js')
const outFile = join(here, '../public/tapp-runtime/three.0.170.iife.js')

await mkdir(dirname(outFile), { recursive: true })
await build({
  entryPoints: [entry],
  bundle: true,
  format: 'iife',
  platform: 'browser',
  target: ['es2020'],
  minify: true,
  outfile: outFile,
  logLevel: 'info',
})
await writeFile(
  join(dirname(outFile), 'README.md'),
  '# Tapp host runtime\n\n`three.0.170.iife.js` is built by `frontend/scripts/bundle-tapp-three.mjs`.\nIt is loaded only when a Page declares `runtimeModules: ["three"]`.\n',
)
console.log(`wrote ${outFile}`)
