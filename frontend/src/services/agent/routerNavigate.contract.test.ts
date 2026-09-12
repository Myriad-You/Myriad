import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '../../..')

function source(rel: string): string {
  return readFileSync(join(root, rel), 'utf8')
}

function parseRouterPrefixes(rust: string): string[] {
  const block = rust.match(
    /pub const VALID_ROUTER_PREFIXES: &\[&str\] = &\[([\s\S]*?)\];/,
  )
  assert.ok(block, 'VALID_ROUTER_PREFIXES must exist')
  return Iterator.from(block[1].matchAll(/"([^"]+)"/g))
    .map((match) => match[1])
    .toArray()
}

function parseAppRoutes(tsx: string): string[] {
  return Iterator.from(tsx.matchAll(/path=["']([^"']+)["']/g))
    .map((match) => match[1])
    .toArray()
}

describe('router.navigate allow-list', () => {
  const prefixes = parseRouterPrefixes(
    source('../backend/src/services/agent/ui_analysis.rs'),
  )
  const appRoutes = parseAppRoutes(source('src/App.tsx'))

  it('allows every live user-facing App route', () => {
    const live = [
      '/',
      '/library',
      '/brew',
      '/reports',
      '/config',
      '/tapp',
      '/setup',
    ]
    for (const route of live) {
      assert.ok(
        appRoutes.some(
          (path) =>
            path === route ||
            path.startsWith(`${route}/`) ||
            path === `${route}/*`,
        ),
        `${route} must exist in App.tsx (got ${appRoutes.join(', ')})`,
      )
      assert.ok(
        prefixes.some(
          (prefix) =>
            prefix === route ||
            (prefix !== '/' && route.startsWith(`${prefix}/`)) ||
            route === prefix,
        ),
        `${route} must be allowed by VALID_ROUTER_PREFIXES (${prefixes.join(', ')})`,
      )
    }
  })

  it('does not allow dead prefixes that 404 to home', () => {
    for (const dead of [
      '/home',
      '/platform',
      '/report',
      '/settings',
      '/profile',
      '/agent',
    ]) {
      assert.ok(
        !prefixes.includes(dead),
        `${dead} is not an App.tsx route and must not be in the allow-list`,
      )
    }
  })
})
