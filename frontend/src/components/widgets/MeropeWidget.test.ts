import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const widget = readFileSync(new URL('./MeropeWidget.tsx', import.meta.url), 'utf8')
const css = readFileSync(new URL('./MeropeWidget.css', import.meta.url), 'utf8')

test('library preview uses the generation style-reference as the portrait', () => {
  assert.match(widget, /STYLE_REFERENCE_PREVIEW = '\/merope\/style-reference\.png'/)
  assert.match(widget, /src=\{STYLE_REFERENCE_PREVIEW\}/)
  assert.match(widget, /band=\{compact \? null : PREVIEW_MOOD_BAND\}/)
})

test('live canvas stays hidden until this mount has presented a frame', () => {
  assert.match(css, /\.merope-widget__rig \.merope-rig canvas \{[^}]*opacity:\s*0/)
  assert.match(
    css,
    /\.merope-widget__rig \.merope-rig\.is-ready canvas \{[^}]*opacity:\s*1/,
  )
  assert.match(widget, /if \(!motionReady\) setReadyKey\(''\)/)
})

test('nameplate follows face presence instead of popping with the live lease', () => {
  assert.match(widget, /\{agentName \? \(/)
  assert.doesNotMatch(widget, /agentName && wantLive/)
  assert.match(widget, /notifyLiveFaceUnmounted\(playbackId\)/)
  assert.match(widget, /hostRef=\{surfaceRef\}/)
  assert.match(
    css,
    /\.merope-widget__surface\[data-face-phase\] \.merope-widget__identity/,
  )
  assert.match(
    css,
    /\.merope-widget__surface\[data-face-phase='enter'\] \.merope-widget__identity/,
  )
  assert.match(
    css,
    /\.merope-widget__surface\[data-face-phase='exit'\] \.merope-widget__identity/,
  )
})

test('the live widget sinks and rises from below with the face', () => {
  const presence = readFileSync(
    new URL('../../features/merope/facePresence.css', import.meta.url),
    'utf8',
  )
  assert.match(presence, /--face-presence-lift:\s*10px/)
  assert.match(
    presence,
    /\.face-presence \{[\s\S]*?translate:\s*0 var\(--face-presence-lift\)/,
  )
  assert.match(widget, /data-rig-quality=/)
  assert.doesNotMatch(css, /:has\(/)
  assert.doesNotMatch(css, /--merope-face-lift/)
})

test('4x4 mood label is secondary to the name and the level ticks', () => {
  assert.match(widget, /className="merope-widget__mood-text"/)
  assert.match(widget, /format\(o\.moodLine, \{ band: word \}\)/)
  assert.match(css, /\.merope-widget__mood\s*\{[^}]*--text-muted/)
  assert.match(
    css,
    /\.merope-widget__mood-text\s*\{[^}]*font-weight:\s*400/,
  )
  assert.doesNotMatch(
    css,
    /\.merope-widget__mood-text\s*\{[^}]*font-weight:\s*[5-9]00/,
  )
})

test('widget preview knocks the studio paper out to alpha', async () => {
  const { default: sharp } = await import('sharp')
  const previewPath = fileURLToPath(
    new URL('../../../public/merope/style-reference.png', import.meta.url),
  )
  const { width, height, channels, hasAlpha } = await sharp(
    previewPath,
  ).metadata()
  assert.equal(width, 1086)
  assert.equal(height, 1448)
  assert.equal(channels, 4)
  assert.equal(hasAlpha, true)

  const { data, info } = await sharp(previewPath)
    .ensureAlpha()
    .raw()
    .toBuffer({ resolveWithObject: true })
  const at = (x: number, y: number) =>
    data[(y * info.width + x) * info.channels + 3]
  assert.equal(at(2, 2), 0)
  assert.equal(at(1083, 2), 0)
  assert.equal(at(543, 80), 255)
  assert.equal(at(543, 1000), 255)
})
