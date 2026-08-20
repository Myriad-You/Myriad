import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  harmonizeGradientPalette,
  hexHueDelta,
  pickGradientCompanion,
} from './colorHarmony.ts'

describe('harmonizeGradientPalette', () => {
  it('pulls complementary brown/blue into the same hue family', () => {
    const out = harmonizeGradientPalette({
      primary: '#8b5a3c',
      secondary: '#4a90c8',
      accent: '#3b82f6',
      light: '#c4a484',
      dark: '#4a2c1a',
    })

    assert.ok(
      hexHueDelta(out.primary, out.secondary) < 70,
      `secondary still clashes: ${out.primary} → ${out.secondary} (${hexHueDelta(out.primary, out.secondary).toFixed(1)}°)`,
    )
    assert.ok(
      hexHueDelta(out.primary, out.accent) < 70,
      `accent still clashes: ${out.primary} → ${out.accent}`,
    )
  })

  it('keeps an already analogous pair close to the source hues', () => {
    const source = {
      primary: '#b45309',
      secondary: '#d97706',
      accent: '#f59e0b',
      light: '#fbbf24',
      dark: '#78350f',
    }
    const out = harmonizeGradientPalette(source)

    assert.ok(
      hexHueDelta(source.primary, out.primary) < 8,
      `primary hue drifted: ${source.primary} → ${out.primary}`,
    )
    assert.ok(
      hexHueDelta(out.primary, out.secondary) < 60,
      `analogous pair was over-corrected: ${out.primary} → ${out.secondary}`,
    )
  })

  it('caps neon complementary stops so they no longer clash', () => {
    const out = harmonizeGradientPalette({
      primary: '#ff00aa',
      secondary: '#00ffcc',
      accent: '#00ffcc',
      light: '#ff99dd',
      dark: '#660044',
    })

    assert.ok(
      hexHueDelta(out.primary, out.secondary) < 70,
      `neon pair still clashes: ${out.primary} → ${out.secondary}`,
    )
  })

  it('keeps a lightness gap so the gradient is not a flat disk', () => {
    const out = harmonizeGradientPalette({
      primary: '#7c3aed',
      secondary: '#7c3aed',
      accent: '#7c3aed',
      light: '#c4b5fd',
      dark: '#4c1d95',
    })

    const parse = (hex: string) => {
      const n = hex.slice(1)
      return {
        r: Number.parseInt(n.slice(0, 2), 16),
        g: Number.parseInt(n.slice(2, 4), 16),
        b: Number.parseInt(n.slice(4, 6), 16),
      }
    }
    const luma = (hex: string) => {
      const { r, g, b } = parse(hex)
      return 0.299 * r + 0.587 * g + 0.114 * b
    }

    assert.ok(
      Math.abs(luma(out.primary) - luma(out.secondary)) > 12,
      `stops too close: ${out.primary} / ${out.secondary}`,
    )
  })
})

describe('pickGradientCompanion', () => {
  it('prefers an analogous cover color over a complementary one', () => {
    const primary = {
      r: 139,
      g: 90,
      b: 60,
      percentage: 40,
      saturation: 0.57,
      brightness: 100,
      chroma: 79,
    }
    const analog = {
      r: 176,
      g: 122,
      b: 68,
      percentage: 12,
      saturation: 0.61,
      brightness: 130,
      chroma: 108,
    }
    const clash = {
      r: 74,
      g: 144,
      b: 200,
      percentage: 28,
      saturation: 0.63,
      brightness: 130,
      chroma: 126,
    }

    const picked = pickGradientCompanion(primary, [clash, analog])
    assert.equal(picked.r, analog.r)
    assert.equal(picked.g, analog.g)
    assert.equal(picked.b, analog.b)
  })

  it('synthesizes a companion when only a complementary color remains', () => {
    const primary = {
      r: 139,
      g: 90,
      b: 60,
      percentage: 40,
      saturation: 0.57,
      brightness: 100,
      chroma: 79,
    }
    const clash = {
      r: 74,
      g: 144,
      b: 200,
      percentage: 28,
      saturation: 0.63,
      brightness: 130,
      chroma: 126,
    }

    const picked = pickGradientCompanion(primary, [clash])
    const primaryHex = `#${[primary.r, primary.g, primary.b].map((v) => v.toString(16).padStart(2, '0')).join('')}`
    const pickedHex = `#${[picked.r, picked.g, picked.b].map((v) => v.toString(16).padStart(2, '0')).join('')}`
    assert.ok(
      hexHueDelta(primaryHex, pickedHex) < 70,
      `synthesized companion still clashes: ${primaryHex} → ${pickedHex}`,
    )
    assert.notEqual(pickedHex, primaryHex)
  })
})
