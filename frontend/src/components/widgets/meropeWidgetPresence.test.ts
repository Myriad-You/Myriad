import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  liveCanvasHidden,
  nameplateHidden,
  previewMoodBand,
  previewPortraitSrc,
  readyKeyAfterMotionChange,
  shouldShowNameplate,
  STYLE_REFERENCE_PREVIEW,
} from './meropeWidgetPresence'

describe('merope widget presence', () => {
  it('uses the style-reference portrait and hides the mood band when compact', () => {
    assert.equal(previewPortraitSrc(), '/merope/style-reference.png')
    assert.equal(previewPortraitSrc(), STYLE_REFERENCE_PREVIEW)
    assert.equal(previewMoodBand(false), 'calm')
    assert.equal(previewMoodBand(undefined), 'calm')
    assert.equal(previewMoodBand(true), null)
  })

  it('shows the nameplate from the name, not from the live lease', () => {
    assert.equal(shouldShowNameplate('Ada'), true)
    assert.equal(shouldShowNameplate(null), false)
    assert.equal(nameplateHidden(false), true)
    assert.equal(nameplateHidden(true), false)
  })

  it('clears the ready key when this mount has not presented a live frame', () => {
    assert.equal(readyKeyAfterMotionChange(false, 'live'), '')
    assert.equal(readyKeyAfterMotionChange(true, 'live'), 'live')
    assert.equal(liveCanvasHidden(false), true)
    assert.equal(liveCanvasHidden(true), false)
  })
})

describe('merope widget CSS contract', () => {
  const css = readFileSync(new URL('./MeropeWidget.css', import.meta.url), 'utf8')
  const presence = readFileSync(
    new URL('../../features/merope/facePresence.css', import.meta.url),
    'utf8',
  )

  it('keeps the live canvas hidden until the ready class lands', () => {
    assert.match(css, /\.merope-widget__rig \.merope-rig canvas/)
    assert.match(css, /\.merope-widget__rig \.merope-rig\.is-ready canvas/)
    assert.match(css, /opacity:\s*0/)
    assert.match(css, /opacity:\s*1/)
  })

  it('rises with the shared face-presence lift, not a local :has() hack', () => {
    assert.match(presence, /--face-presence-lift:\s*10px/)
    assert.doesNotMatch(css, /:has\(/)
    assert.doesNotMatch(css, /--merope-face-lift/)
  })

  it('keeps the 4x4 mood label secondary to the name', () => {
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
})

describe('style-reference preview asset', () => {
  it('knocks the studio paper out to alpha', async () => {
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
})
