import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))
const panelCss = readFileSync(join(dir, 'agent-panel.css'), 'utf8')
const themeCss = readFileSync(join(dir, '../../styles/theme.css'), 'utf8')

it('neither agent-panel.css nor theme.css references undefined --surface or --surface-solid tokens', () => {
  const undefinedSurfaceToken = /--surface(?:-solid)?(?![\w-])/
  assert.doesNotMatch(panelCss, undefinedSurfaceToken)
  assert.doesNotMatch(themeCss, undefinedSurfaceToken)
})

it('theme.css defines --color-danger and --color-error in both :root and html.dark', () => {
  const rootMatch = themeCss.match(/:root\s*\{([\s\S]*?)\n\}/)
  assert.ok(rootMatch, ':root block should exist in theme.css')
  assert.match(rootMatch[1], /--color-danger:\s*#dc2626;/)
  assert.match(rootMatch[1], /--color-error:\s*#dc2626;/)

  const darkMatch = themeCss.match(/html\.dark\s*\{([\s\S]*?)\n\}/)
  assert.ok(darkMatch, 'html.dark block should exist in theme.css')
  assert.match(darkMatch[1], /--color-danger:\s*#f87171;/)
  assert.match(darkMatch[1], /--color-error:\s*#f87171;/)
})

it('chat-mode site search stacks above the face slot and hides the face reserve', () => {
  assert.match(
    panelCss,
    /\.agent-panel-search,[^{]*\{[^}]*position:\s*relative;[^}]*z-index:\s*3;/,
  )
  assert.match(
    panelCss,
    /\.agent-panel-composer\[data-mode='chat'\]:has\(>\s*\.agent-panel-search\)\s*\{[^}]*padding-top:\s*0;/,
  )
  assert.match(
    panelCss,
    /\.agent-panel-composer\[data-mode='chat'\]:has\(>\s*\.agent-panel-search\)\s+\.agent-panel-face-slot\s*\{[^}]*opacity:\s*0;[^}]*pointer-events:\s*none;/,
  )
  assert.match(panelCss, /html \.agent-panel-search\.glass,[^{]*\{[^}]*backdrop-filter:\s*none;/)
  assert.match(
    panelCss,
    /\.agent-panel-search-row:focus-visible\s*\{[^}]*outline:\s*2px solid var\(--cfg-accent,\s*#10b981\);[^}]*outline-offset:\s*-2px;/,
  )
})

it('card pointer and away banner use surface tokens for legibility', () => {
  assert.match(
    panelCss,
    /\.agent-panel-card-pointer\s*\{[^}]*background:\s*var\(--surface-bg,/,
  )
  assert.match(
    panelCss,
    /\.agent-panel-away\s*\{[^}]*margin:\s*0;[^}]*background:\s*var\(--surface-bg,/,
  )
})
