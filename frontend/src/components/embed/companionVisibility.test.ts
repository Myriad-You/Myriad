import assert from 'node:assert/strict'
import { beforeEach, describe, it, mock } from 'node:test'
import { AFTER_LEAVE_MS, ChromeVisibility, IDLE_MS, OWN_SCROLL_MS } from './companionVisibility.ts'

describe('两侧胶囊的显隐', () => {
  let clock = 0
  beforeEach(() => {
    mock.timers.reset()
    mock.timers.enable({ apis: ['setTimeout'] })
    clock = 0
  })
  const tick = (ms: number) => {
    clock += ms
    mock.timers.tick(ms)
  }
  const make = (y = 500) => {
    const changes: boolean[] = []
    const chrome = new ChromeVisibility(y, (visible) => changes.push(visible), () => clock)
    return { chrome, changes }
  }
  /** 一像素一像素地滚，像触控板那样。 */
  const glide = (chrome: ChromeVisibility, from: number, to: number) => {
    const step = from < to ? 1 : -1
    for (let y = from + step; y !== to + step; y += step) chrome.scroll(y)
  }

  it('打开后停 4 秒收起', () => {
    const { chrome } = make()
    tick(IDLE_MS - 1)
    assert.equal(chrome.visible, true)
    tick(1)
    assert.equal(chrome.visible, false)
  })

  it('慢慢往下读也会收起，慢慢往上翻也会出来', () => {
    const { chrome } = make()
    glide(chrome, 500, 520)
    assert.equal(chrome.visible, true)
    glide(chrome, 520, 530)
    assert.equal(chrome.visible, false)
    glide(chrome, 530, 510)
    assert.equal(chrome.visible, false)
    glide(chrome, 510, 505)
    assert.equal(chrome.visible, true)
    tick(AFTER_LEAVE_MS)
    assert.equal(chrome.visible, false)
  })

  it('往回一点又接着往下，不会误判成往上翻', () => {
    const { chrome } = make()
    glide(chrome, 500, 530)
    glide(chrome, 530, 520)
    glide(chrome, 520, 600)
    assert.equal(chrome.visible, false)
  })

  it('回到页面顶端一直在', () => {
    const { chrome } = make()
    glide(chrome, 500, 540)
    assert.equal(chrome.visible, false)
    chrome.scroll(60)
    assert.equal(chrome.visible, true)
    tick(IDLE_MS * 3)
    assert.equal(chrome.visible, true)
  })

  it('在顶端打开不按时收起，往下读开始后照常', () => {
    const { chrome } = make(0)
    tick(IDLE_MS * 3)
    assert.equal(chrome.visible, true)
    chrome.hold(true)
    chrome.hold(false)
    tick(AFTER_LEAVE_MS * 3)
    assert.equal(chrome.visible, true)
    glide(chrome, 0, 110)
    assert.equal(chrome.visible, false)
  })

  it('离开顶端停下来没读够也会按时收起', () => {
    const { chrome } = make(0)
    glide(chrome, 0, 95)
    glide(chrome, 95, 90)
    glide(chrome, 90, 100)
    assert.equal(chrome.visible, true)
    tick(IDLE_MS)
    assert.equal(chrome.visible, false)
  })

  it('伸手到胶囊那一带就出来，停在上面不收', () => {
    const { chrome } = make()
    tick(IDLE_MS)
    assert.equal(chrome.visible, false)
    chrome.reach()
    assert.equal(chrome.visible, true)
    chrome.hold(true)
    glide(chrome, 500, 700)
    tick(IDLE_MS * 3)
    assert.equal(chrome.visible, true)
    chrome.hold(false)
    tick(AFTER_LEAVE_MS)
    assert.equal(chrome.visible, false)
  })

  it('自己滚动页面的那一阵不算读者在滚', () => {
    const { chrome } = make()
    chrome.ownScroll()
    glide(chrome, 500, 1500)
    assert.equal(chrome.visible, true)
    tick(OWN_SCROLL_MS)
    glide(chrome, 1500, 1530)
    assert.equal(chrome.visible, false)
  })

  it('只在真变了的时候通知', () => {
    const { chrome, changes } = make()
    chrome.reach()
    chrome.reach()
    tick(IDLE_MS)
    chrome.reach()
    assert.deepEqual(changes, [false, true])
  })
})
