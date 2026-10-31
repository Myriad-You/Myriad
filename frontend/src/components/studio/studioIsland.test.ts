import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { blogReloadDecision, hideOverlayOnPageSwap, withoutStudioParam } from './studioIsland.ts'

describe('hideOverlayOnPageSwap', () => {
  const setup = () => {
    const top = new EventTarget() as unknown as Window
    const style: Record<string, unknown> = {
      removeProperty: (name: string) => delete style[name],
    }
    const props = { get: (name: string) => style[name] }
    const host = { style } as unknown as HTMLElement
    const fire = (type: string, extra: object) => top.dispatchEvent(Object.assign(new Event(type), extra))
    return { top, host, props, fire }
  }

  it('有换页画面时拍旧页之前收掉，往返缓存拿回来再放出来', () => {
    const { top, host, props, fire } = setup()
    const stop = hideOverlayOnPageSwap(top, host)
    fire('pageswap', { viewTransition: null })
    assert.equal(props.get('display'), undefined)
    fire('pageswap', { viewTransition: {} })
    assert.equal(props.get('display'), 'none')
    fire('pageshow', { persisted: false })
    assert.equal(props.get('display'), 'none')
    fire('pageshow', { persisted: true })
    assert.equal(props.get('display'), undefined)
    stop()
    fire('pageswap', { viewTransition: {} })
    assert.equal(props.get('display'), undefined)
  })
})

describe('blogReloadDecision', () => {
  const ok = (at: string | null) => ({ status: 'ok', lastBuiltAt: at })

  it('博客重新生成过才刷新外层页面', () => {
    assert.equal(blogReloadDecision(ok('a'), ok('b')), 'reload')
    assert.equal(blogReloadDecision(ok('a'), ok('a')), 'stay')
    assert.equal(blogReloadDecision(ok(null), ok('a')), 'reload')
  })

  it('还在生成就等，拿不到打开时的状态就不动', () => {
    assert.equal(blogReloadDecision(ok('a'), { status: 'running', lastBuiltAt: 'a' }), 'wait')
    assert.equal(blogReloadDecision(ok('a'), { status: 'queued', lastBuiltAt: 'a' }), 'wait')
    assert.equal(blogReloadDecision(null, ok('b')), 'stay')
  })
})

describe('withoutStudioParam', () => {
  it('只去掉 studio，别的参数和路径不动', () => {
    assert.equal(
      withoutStudioParam('http://x.test/blog/posts/a/?studio=/notes/3&page=2'),
      'http://x.test/blog/posts/a/?page=2',
    )
    assert.equal(withoutStudioParam('http://x.test/blog/?studio=/'), 'http://x.test/blog/')
    assert.equal(withoutStudioParam('http://x.test/blog/'), 'http://x.test/blog/')
  })
})
