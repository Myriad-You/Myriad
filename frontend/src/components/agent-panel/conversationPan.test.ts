import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  clampConversationScroll,
  conversationExitKey,
  conversationExitStyle,
  conversationHoldExitOnClose,
  conversationMaxScroll,
  conversationShellLimit,
  conversationViewHeight,
  decayVelocity,
  rubberband,
  sampleVelocity,
  smoothToward,
  stillCoasting,
  wheelDeltaY,
} from './conversationPan'

describe('conversationMaxScroll', () => {
  it('is zero when the thread fits', () => {
    assert.equal(conversationMaxScroll(200, 400), 0)
  })

  it('is the extra length when the thread is taller than the viewport', () => {
    assert.equal(conversationMaxScroll(900, 400), 500)
  })
})

describe('conversationViewHeight', () => {
  it('does not treat the whole track as the window when the shell is unknown', () => {
    assert.equal(conversationViewHeight(800, 0), 0)
  })

  it('caps at the space above the composer', () => {
    assert.equal(conversationViewHeight(1200, 400), 400)
  })

  it('does not invent extra room when the thread is short', () => {
    assert.equal(conversationViewHeight(120, 400), 120)
  })
})

describe('conversationShellLimit', () => {
  it('uses the resolved max-height when the browser gave pixels', () => {
    assert.equal(conversationShellLimit(800, 900), 800)
  })

  it('falls back to the viewport when max-height is none', () => {
    assert.equal(conversationShellLimit(Number.NaN, 900), 868)
  })
})

describe('clampConversationScroll', () => {
  it('stays inside [0, max]', () => {
    assert.equal(clampConversationScroll(-12, 80), 0)
    assert.equal(clampConversationScroll(12, 80), 12)
    assert.equal(clampConversationScroll(120, 80), 80)
    assert.equal(clampConversationScroll(12, 0), 0)
  })
})

describe('wheelDeltaY', () => {
  it('keeps pixel deltas', () => {
    assert.equal(wheelDeltaY(40, 0), 40)
  })

  it('turns line and page modes into pixels', () => {
    assert.equal(wheelDeltaY(2, 1), 32)
    assert.equal(wheelDeltaY(1, 2), 800)
  })
})

describe('smoothToward', () => {
  it('settles when already close', () => {
    assert.equal(smoothToward(10.1, 10, 0.016), 10)
  })

  it('moves a fraction of the remaining gap', () => {
    const next = smoothToward(0, 100, 0.016, 0.028)
    assert.ok(next > 40 && next < 50)
  })
})

describe('decayVelocity', () => {
  it('kills a crawl', () => {
    assert.equal(decayVelocity(40, 0.016), 0)
  })

  it('keeps a fling moving', () => {
    const next = decayVelocity(1400, 0.016)
    assert.ok(next > 1200 && next < 1400)
  })
})

describe('sampleVelocity', () => {
  it('reads px/s from recent samples', () => {
    const v = sampleVelocity(
      [
        { t: 1000, x: 0 },
        { t: 1080, x: 80 },
      ],
      1080,
    )
    assert.equal(v, 1000)
  })
})

describe('rubberband', () => {
  it('does not stretch inside the range', () => {
    assert.equal(rubberband(40, 200, 400), 40)
  })

  it('resists past the ends', () => {
    assert.ok(rubberband(-80, 200, 400) > -80)
    assert.ok(rubberband(280, 200, 400) < 280)
  })
})

describe('stillCoasting', () => {
  it('keeps the loop alive while catching up or flung', () => {
    assert.equal(stillCoasting(0, 40, 0, false), true)
    assert.equal(stillCoasting(40, 40, 400, false), true)
    assert.equal(stillCoasting(40, 40, 0, false), false)
    assert.equal(stillCoasting(40, 40, 0, true), true)
  })
})

describe('conversationExitStyle', () => {
  const viewportTop = 100
  const viewportBottom = 500
  const fade = 80

  it('leaves a fully visible card alone', () => {
    assert.deepEqual(
      conversationExitStyle(180, 240, viewportTop, viewportBottom, fade),
      { exit: 0, shift: 0, hidden: false },
    )
  })

  it('hides a card that has left through the top', () => {
    const style = conversationExitStyle(
      20,
      80,
      viewportTop,
      viewportBottom,
      fade,
    )
    assert.equal(style.hidden, true)
    assert.equal(style.exit, 1)
    assert.ok(style.shift < 0)
  })

  it('hides a card that has left through the bottom', () => {
    const style = conversationExitStyle(
      520,
      580,
      viewportTop,
      viewportBottom,
      fade,
    )
    assert.equal(style.hidden, true)
    assert.equal(style.exit, 1)
    assert.ok(style.shift > 0)
  })

  it('keeps a tall card solid while most of it is still in view', () => {
    const style = conversationExitStyle(
      40,
      400,
      viewportTop,
      viewportBottom,
      fade,
    )
    assert.deepEqual(style, { exit: 0, shift: 0, hidden: false })
  })

  it('fades a short card as a whole when it starts to leave the top', () => {
    const style = conversationExitStyle(
      60,
      140,
      viewportTop,
      viewportBottom,
      fade,
    )
    assert.equal(style.hidden, false)
    assert.ok(style.exit > 0.45 && style.exit < 0.55)
    assert.ok(style.shift < 0)
  })

  it('fades a short card as a whole when it starts to leave the bottom', () => {
    const style = conversationExitStyle(
      460,
      540,
      viewportTop,
      viewportBottom,
      fade,
    )
    assert.equal(style.hidden, false)
    assert.ok(style.exit > 0.45 && style.exit < 0.55)
    assert.ok(style.shift > 0)
  })

  it('fades the last remnant of a tall card as a whole', () => {
    const style = conversationExitStyle(
      20,
      140,
      viewportTop,
      viewportBottom,
      fade,
    )
    assert.equal(style.hidden, false)
    assert.ok(style.exit > 0.45 && style.exit < 0.55)
    assert.ok(style.shift < 0)
  })

  it('freezes overflow fade when the panel closes, instead of restoring a solid card', () => {
    const fading = conversationExitStyle(
      60,
      140,
      viewportTop,
      viewportBottom,
      fade,
    )
    assert.equal(conversationHoldExitOnClose(fading), true)
    const gone = conversationExitStyle(
      20,
      80,
      viewportTop,
      viewportBottom,
      fade,
    )
    assert.equal(conversationHoldExitOnClose(gone), true)
    const inView = conversationExitStyle(
      180,
      240,
      viewportTop,
      viewportBottom,
      fade,
    )
    assert.equal(conversationHoldExitOnClose(inView), false)
    const tallStillReading = conversationExitStyle(
      40,
      400,
      viewportTop,
      viewportBottom,
      fade,
    )
    assert.equal(conversationHoldExitOnClose(tallStillReading), false)
  })

  it('skips redundant writes with a stable key', () => {
    const a = conversationExitStyle(40, 400, viewportTop, viewportBottom, fade)
    const b = conversationExitStyle(40, 400, viewportTop, viewportBottom, fade)
    assert.equal(conversationExitKey(a), conversationExitKey(b))
    assert.equal(conversationExitKey({ exit: 0, shift: 0, hidden: false }), 'r')
    assert.notEqual(
      conversationExitKey({ exit: 0, shift: 0, hidden: false }),
      '',
    )
    assert.equal(
      conversationExitKey({ exit: 1, shift: -16, hidden: true }),
      'h',
    )
  })
})
