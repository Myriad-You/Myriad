#!/usr/bin/env node
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'
/**
 * First-paint JS/CSS budget for the home shell.
 * Counts compressed (gzip) bytes of assets referenced by index.html plus
 * statically imported chunks. Agora / Config route chunks must stay out of
 * that set.
 */
import { promisify } from 'node:util'
import { gzip } from 'node:zlib'

const gzipAsync = promisify(gzip)
const here = dirname(fileURLToPath(import.meta.url))
const frontendRoot = resolve(here, '..')
const distDir = resolve(frontendRoot, 'dist')
const baselinePath = resolve(here, 'home-budget.baseline.json')

const SLACK = 0.15

function findIndexHtml(root = distDir) {
  const candidates = [
    join(root, 'index.html'),
    join(root, 'client', 'index.html'),
  ]
  return candidates.find((path) => existsSync(path)) ?? null
}

function collectReferencedAssets(html, htmlDir) {
  const assets = new Set()
  const patterns = [
    /(?:src|href|component-url|renderer-url)=["']([^"']+\.(?:js|css))["']/g,
    /<link[^>]+href=["']([^"']+\.(?:js|css))["']/g,
  ]
  for (const pattern of patterns) {
    for (const match of html.matchAll(pattern)) {
      const href = match[1]
      if (href.startsWith('http') || href.startsWith('data:')) continue
      const abs = resolve(htmlDir, href.replaceAll(/^\//g, ''))
      if (existsSync(abs)) assets.add(abs)
    }
  }
  return assets
}

function displayPath(file, root) {
  if (file.startsWith(root)) return file.slice(root.length + 1)
  if (file.startsWith(frontendRoot)) return file.slice(frontendRoot.length + 1)
  return file
}

function walkStaticImports(entryFiles, assetsDir) {
  const seen = new Set(entryFiles)
  const queue = Iterator.from(entryFiles).toArray()
  const importRe =
    /(?:from|import)\s*["'](\.{0,2}\/[^"']+\.js)["']|import\(["'](\.{0,2}\/[^"']+\.js)["']\)/g
  while (queue.length > 0) {
    const file = queue.pop()
    if (!file.endsWith('.js') || !existsSync(file)) continue
    const source = readFileSync(file, 'utf8')
    for (const match of source.matchAll(importRe)) {
      const spec = match[1] || match[2]
      if (!spec) continue
      // Dynamic import() of Config / Agora must not be followed as first-paint.
      const isDynamic = match[0].startsWith('import(')
      if (isDynamic) continue
      const next = resolve(dirname(file), spec)
      if (!seen.has(next) && existsSync(next)) {
        seen.add(next)
        queue.push(next)
      }
    }
    void assetsDir
  }
  return seen
}

async function gzipSize(path) {
  const buf = await gzipAsync(readFileSync(path))
  return buf.byteLength
}

export async function measureHomeBudget(root = distDir) {
  const htmlPath = findIndexHtml(root)
  if (!htmlPath) {
    throw new Error(`home budget: no index.html under ${root}`)
  }
  const html = readFileSync(htmlPath, 'utf8')
  const htmlDir = dirname(htmlPath)
  const referenced = collectReferencedAssets(html, htmlDir)
  const assetsDir = existsSync(join(htmlDir, 'assets'))
    ? join(htmlDir, 'assets')
    : join(root, 'assets')
  const firstPaint = walkStaticImports(Iterator.from(referenced).toArray(), assetsDir)

  let js = 0
  let css = 0
  const files = []
  for (const file of firstPaint) {
    const size = await gzipSize(file)
    files.push({ file: displayPath(file, root), gzip: size })
    if (file.endsWith('.css')) css += size
    else js += size
  }
  const ranked = files.toSorted((a, b) => b.gzip - a.gzip)

  const blob = Iterator.from(firstPaint)
    .filter((file) => file.endsWith('.js'))
    .map((file) => readFileSync(file, 'utf8'))
    .toArray()
    .join('\n')

  return {
    jsGzipBytes: js,
    cssGzipBytes: css,
    totalGzipBytes: js + css,
    files: ranked,
    loadsAgora: /agora-rtc-sdk-ng|agora-rtm/.test(blob),
    // Filename, not a lazy-import string left inside App.
    loadsConfigRoute: Iterator.from(firstPaint).some((file) =>
      /(?:^|\/)Config-[^/]+\.js$/.test(file),
    ),
  }
}

function loadBaseline() {
  return JSON.parse(readFileSync(baselinePath, 'utf8'))
}

function withinSlack(actual, baseline) {
  return actual <= Math.ceil(baseline * (1 + SLACK))
}

async function main() {
  const write = process.argv.includes('--write')
  const check = process.argv.includes('--check') || !write
  if (!existsSync(distDir)) {
    console.error('home budget: dist/ missing; run pnpm build first')
    process.exit(2)
  }
  const measured = await measureHomeBudget()
  if (write) {
    writeFileSync(
      baselinePath,
      `${JSON.stringify(
        {
          jsGzipBytes: measured.jsGzipBytes,
          cssGzipBytes: measured.cssGzipBytes,
          totalGzipBytes: measured.totalGzipBytes,
        },
        null,
        2,
      )}\n`,
    )
    console.log(`wrote ${baselinePath}`)
  }
  console.log(
    JSON.stringify(
      {
        jsGzipBytes: measured.jsGzipBytes,
        cssGzipBytes: measured.cssGzipBytes,
        totalGzipBytes: measured.totalGzipBytes,
        loadsAgora: measured.loadsAgora,
        loadsConfigRoute: measured.loadsConfigRoute,
        top: measured.files.slice(0, 8),
      },
      null,
      2,
    ),
  )
  if (check && existsSync(baselinePath)) {
    const baseline = loadBaseline()
    const failures = []
    if (!withinSlack(measured.jsGzipBytes, baseline.jsGzipBytes)) {
      failures.push(
        `JS gzip ${measured.jsGzipBytes} exceeds baseline ${baseline.jsGzipBytes} +15%`,
      )
    }
    if (!withinSlack(measured.cssGzipBytes, baseline.cssGzipBytes)) {
      failures.push(
        `CSS gzip ${measured.cssGzipBytes} exceeds baseline ${baseline.cssGzipBytes} +15%`,
      )
    }
    if (measured.loadsAgora) {
      failures.push('first-paint JS contains Agora SDK')
    }
    if (measured.loadsConfigRoute) {
      failures.push('first-paint JS contains the Config route')
    }
    if (failures.length > 0) {
      console.error(failures.join('\n'))
      process.exit(1)
    }
  }
}

const invoked = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)
if (invoked) {
  main().catch((error) => {
    console.error(error)
    process.exit(1)
  })
}
