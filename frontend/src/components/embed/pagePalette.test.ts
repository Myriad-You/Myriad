import assert from 'node:assert/strict'
import { test } from 'node:test'
import { contrast, derivePalette, KIND_HUES, luminance, over, parseColor } from './pagePalette.ts'

function c(value: string) {
  const color = parseColor(value)
  assert.ok(color, value)
  return color
}

/** 真实主题在浏览器里量到的文章页底色和正文色。 */
const THEMES = {
  'myriad-default light': ['rgb(247, 245, 242)', 'rgb(87, 83, 78)'],
  'myriad-default dark': ['rgb(28, 25, 23)', 'rgb(168, 162, 158)'],
  'congo light': ['rgb(255, 255, 255)', 'rgb(63, 63, 70)'],
  'congo dark': ['rgb(39, 39, 42)', 'rgb(212, 212, 216)'],
  'PaperMod light': ['rgb(255, 255, 255)', 'rgb(31, 31, 31)'],
  'PaperMod dark': ['rgb(29, 30, 32)', 'rgb(196, 196, 197)'],
  'stack light': ['rgb(245, 245, 250)', 'rgb(0, 0, 0)'],
  'stack dark': ['rgb(48, 48, 48)', 'rgba(255, 255, 255, 0.9)'],
  'blowfish light': ['rgb(255, 255, 255)', 'rgb(51, 65, 85)'],
  'blowfish dark': ['rgb(30, 41, 59)', 'rgb(203, 213, 225)'],
  'ananke light': ['rgb(244, 244, 244)', 'rgb(0, 0, 0)'],
  // Ananke 的页头是黑底白字：创作台细条在它上面。
  'ananke header': ['rgb(0, 0, 0)', 'rgb(255, 255, 255)'],
} as const

const SITE_ACCENT = c('#635198')

test('认计算样式里的几种写法', () => {
  assert.deepEqual(c('rgb(1, 2, 3)'), { r: 1, g: 2, b: 3, a: 1 })
  assert.deepEqual(c('rgba(255, 255, 255, 0.9)'), { r: 255, g: 255, b: 255, a: 0.9 })
  assert.deepEqual(c('rgb(1 2 3 / 50%)'), { r: 1, g: 2, b: 3, a: 0.5 })
  assert.deepEqual(c('#0f0'), { r: 0, g: 255, b: 0, a: 1 })
  assert.equal(parseColor('oklch(0.5 0.1 200)'), null)
  assert.equal(c('transparent').a, 0)
})

test('每个主题上都看得清', () => {
  for (const [name, [bg, fg]] of Object.entries(THEMES)) {
    const background = c(bg)
    const { vars } = derivePalette({ background, text: c(fg), accent: SITE_ACCENT })
    const solid = c(vars.solid)
    assert.ok(contrast(c(vars.fg), solid) >= 7, `${name} 正文 ${vars.fg} on ${vars.solid}`)
    assert.ok(contrast(c(vars.soft), solid) >= 4.5, `${name} 次要文字 ${vars.soft}`)
    assert.ok(contrast(c(vars.accent), solid) >= 3, `${name} 强调色 ${vars.accent}`)
    assert.ok(contrast(c(vars['on-accent']), c(vars.accent)) >= 4.5, `${name} 强调色上的字`)
    assert.ok(contrast(c(vars.ai), solid) >= 4.5, `${name} AI 色`)
    for (const kind of Object.keys(KIND_HUES)) {
      assert.ok(contrast(c(vars[`kind-${kind}`]), c(vars[`kind-${kind}-bg`])) >= 4.5, `${name} ${kind}`)
    }
  }
})

test('深浅看页面自己，不看主站', () => {
  for (const [name, [bg, fg]] of Object.entries(THEMES)) {
    const { dark } = derivePalette({ background: c(bg), text: c(fg) })
    assert.equal(dark, name.endsWith('dark') || name.endsWith('header'), name)
  }
})

test('面板比页面高一层：浅色不更暗，深色更亮', () => {
  for (const [name, [bg, fg]] of Object.entries(THEMES)) {
    const background = c(bg)
    const { dark, vars } = derivePalette({ background, text: c(fg) })
    const lift = luminance(c(vars.solid)) - luminance(background)
    if (dark) assert.ok(lift > 0.005, `${name} ${vars.solid}`)
    else assert.ok(lift >= -0.001, `${name} ${vars.solid}`)
  }
})

test('带着主题的色调：暖的还暖、冷的还冷、藏青还藏青', () => {
  const warmth = (value: string) => {
    const color = c(value)
    return color.r - color.b
  }
  const solid = (key: keyof typeof THEMES) => derivePalette({ background: c(THEMES[key][0]), text: c(THEMES[key][1]) }).vars.solid
  assert.ok(warmth(solid('myriad-default light')) > 0)
  assert.ok(warmth(solid('myriad-default dark')) > 0)
  assert.ok(warmth(solid('stack light')) < 0)
  assert.ok(warmth(solid('blowfish dark')) < -10)
  assert.equal(warmth(solid('PaperMod light')), 0)
})

test('背后是图片时看文字色定深浅', () => {
  const photo = c('rgb(120, 90, 60)')
  assert.equal(derivePalette({ background: photo, text: c('#fff'), image: true }).dark, true)
  assert.equal(derivePalette({ background: photo, text: c('#111'), image: true }).dark, false)
})

test('半透明的层叠在下面一层上', () => {
  assert.deepEqual(over(c('rgba(0, 0, 0, 0.5)'), c('#fff')), { r: 127.5, g: 127.5, b: 127.5, a: 1 })
})
