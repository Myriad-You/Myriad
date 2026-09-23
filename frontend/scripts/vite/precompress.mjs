import { readdir, readFile, stat, writeFile } from 'node:fs/promises'
import { availableParallelism } from 'node:os'
import path from 'node:path'
import { promisify } from 'node:util'
import zlib from 'node:zlib'

const brotli = promisify(zlib.brotliCompress)
const gzip = promisify(zlib.gzip)

/**
 * Text assets worth a sibling `.br` / `.gz`. HTML and the web manifest are
 * stamped per request by spa-server, so they are compressed there instead.
 */
export const PRECOMPRESSIBLE = /\.(?:js|mjs|css|svg|json|txt|wasm)$/i
const MIN_BYTES = 1024
// A variant must save at least this much to be worth a disk read + header.
const MAX_RATIO = 0.9

async function* walk(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const abs = path.join(dir, entry.name)
    if (entry.isDirectory()) yield* walk(abs)
    else if (entry.isFile() && PRECOMPRESSIBLE.test(entry.name)) yield abs
  }
}

async function precompressFile(file) {
  const { size } = await stat(file)
  if (size < MIN_BYTES) return 0
  const raw = await readFile(file)
  const [br, gz] = await Promise.all([
    brotli(raw, {
      params: {
        [zlib.constants.BROTLI_PARAM_QUALITY]: zlib.constants.BROTLI_MAX_QUALITY,
        [zlib.constants.BROTLI_PARAM_SIZE_HINT]: size,
      },
    }),
    gzip(raw, { level: zlib.constants.Z_BEST_COMPRESSION }),
  ])
  let written = 0
  if (br.length <= size * MAX_RATIO) {
    await writeFile(`${file}.br`, br)
    written++
  }
  if (gz.length <= size * MAX_RATIO) {
    await writeFile(`${file}.gz`, gz)
    written++
  }
  return written
}

/**
 * Docker serves dist through spa-server behind a pass-through proxy; neither
 * compresses on the fly. Emitting variants at build time keeps the runtime a
 * plain file read and gives brotli-11 ratios no per-request budget could afford.
 */
export function precompressPlugin() {
  return {
    name: 'myriad:precompress',
    apply: 'build',
    async writeBundle(options) {
      if (this.environment && this.environment.name !== 'client') return
      const outDir = options.dir
      if (!outDir) return
      const files = await Array.fromAsync(walk(outDir))
      let next = 0
      let variants = 0
      const worker = async () => {
        while (next < files.length) {
          // Not `variants += await …`: that reads `variants` before yielding.
          const written = await precompressFile(files[next++])
          variants += written
        }
      }
      await Promise.all(
        Array.from({ length: Math.max(1, availableParallelism() - 1) }, worker),
      )
      this.environment?.logger.info(
        `[precompress] ${variants} .br/.gz variants for ${files.length} files`,
      )
    },
  }
}
