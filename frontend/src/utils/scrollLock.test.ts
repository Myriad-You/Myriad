import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'

function installDom({ scrollbar = 0 } = {}) {
  const html = { style: { overflow: '' }, clientWidth: 1000 - scrollbar }
  const body = { style: { overflow: '', paddingRight: '' } }
  ;(globalThis as Record<string, unknown>).document = {
    documentElement: html,
    body,
  }
  ;(globalThis as Record<string, unknown>).window = { innerWidth: 1000 }
  return { html, body }
}

afterEach(() => {
  delete (globalThis as Record<string, unknown>).document
  delete (globalThis as Record<string, unknown>).window
})

async function freshLockScroll() {
  const mod = await import(`./scrollLock?t=${Math.random()}`)
  return mod.lockScroll as () => () => void
}

describe('lockScroll', () => {
  it('锁的是 html —— 本站滚动容器是它，只锁 body 页面照滚', async () => {
    const { html, body } = installDom()
    const lockScroll = await freshLockScroll()

    const release = lockScroll()
    assert.equal(html.style.overflow, 'hidden')
    assert.equal(body.style.overflow, 'hidden')

    release()
    assert.equal(html.style.overflow, '')
    assert.equal(body.style.overflow, '')
  })

  it('还原的是锁之前的原值，不是硬写空串', async () => {
    const { html, body } = installDom()
    html.style.overflow = 'hidden auto'
    body.style.overflow = 'clip'
    const lockScroll = await freshLockScroll()

    const release = lockScroll()
    assert.equal(html.style.overflow, 'hidden')
    release()
    assert.equal(html.style.overflow, 'hidden auto')
    assert.equal(body.style.overflow, 'clip')
  })

  it('补滚动条宽度，避免内容横向跳一下', async () => {
    const { body } = installDom({ scrollbar: 15 })
    const lockScroll = await freshLockScroll()

    const release = lockScroll()
    assert.equal(body.style.paddingRight, '15px')
    release()
    assert.equal(body.style.paddingRight, '')
  })

  it('覆盖式滚动条（宽度 0）不补 padding', async () => {
    const { body } = installDom({ scrollbar: 0 })
    const lockScroll = await freshLockScroll()
    const release = lockScroll()
    assert.equal(body.style.paddingRight, '')
    release()
  })

  it('嵌套时内层解锁不能放开外层', async () => {
    const { html } = installDom()
    const lockScroll = await freshLockScroll()

    const releaseOuter = lockScroll()
    const releaseInner = lockScroll()

    releaseInner()
    assert.equal(html.style.overflow, 'hidden', '内层解锁后仍应锁着')

    releaseOuter()
    assert.equal(html.style.overflow, '')
  })

  it('重复调用同一个解锁函数不会把计数减穿', async () => {
    const { html } = installDom()
    const lockScroll = await freshLockScroll()

    const releaseA = lockScroll()
    const releaseB = lockScroll()
    releaseA()
    releaseA()
    releaseA()
    assert.equal(html.style.overflow, 'hidden', 'B 还没释放，不该解锁')

    releaseB()
    assert.equal(html.style.overflow, '')
  })
})
