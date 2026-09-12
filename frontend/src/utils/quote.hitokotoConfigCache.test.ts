/**
 * `/config/hitokoto` 按页去重；例外路径必须回源 / 必须失效。
 * quote.ts 模块加载会挂 HITOKOTO_CONFIG_UPDATED_EVENT（有 window 时），垫片必须先于 import。
 */

import assert from 'node:assert/strict'
import { beforeEach, describe, it } from 'node:test'

// 必须先于 ./quote 的模块体执行
const windowShim = new EventTarget()
;(globalThis as { window?: unknown }).window = windowShim

const apiService = (await import('../services/api')).default
const {
  clearHitokotoConfigCache,
  fetchHitokotoConfig,
  HITOKOTO_CONFIG_UPDATED_EVENT,
} = await import('./quote')

const realGet = apiService.get

/** 替换 apiService.get，记录调用次数并返回指定 sourceId */
function stubConfig(sourceId: string) {
  const state = { calls: 0 }
  apiService.get = (async () => {
    state.calls += 1
    return { success: true, config: { sourceId } }
  }) as typeof apiService.get
  return state
}

describe('fetchHitokotoConfig 去重', () => {
  beforeEach(() => {
    apiService.get = realGet
    clearHitokotoConfigCache()
  })

  it('多个调用方在 TTL 内只回源一次', async () => {
    const stub = stubConfig('hitokoto-anime')

    const first = await fetchHitokotoConfig()
    const second = await fetchHitokotoConfig()
    const third = await fetchHitokotoConfig()

    assert.equal(stub.calls, 1, '三次读取应只有一次网络请求')
    assert.equal(first.sourceId, 'hitokoto-anime')
    assert.equal(second.sourceId, 'hitokoto-anime')
    assert.equal(third.sourceId, 'hitokoto-anime')
  })

  it('并发调用合并为同一次在途请求', async () => {
    const stub = stubConfig('quotable-en')

    const results = await Promise.all([
      fetchHitokotoConfig(),
      fetchHitokotoConfig(),
      fetchHitokotoConfig(),
    ])

    assert.equal(stub.calls, 1, '并发读取应合并成一次请求')
    for (const r of results) assert.equal(r.sourceId, 'quotable-en')
  })

  it('force 绕过缓存（配置编辑器要权威值）', async () => {
    const stub = stubConfig('hitokoto-cn')

    await fetchHitokotoConfig()
    await fetchHitokotoConfig({ force: true })

    assert.equal(stub.calls, 2, 'force 必须真的回源')
  })

  it('force 拿到的新值会写回缓存', async () => {
    const stale = stubConfig('hitokoto-cn')
    await fetchHitokotoConfig()
    assert.equal(stale.calls, 1)

    const fresh = stubConfig('meigen-ja')
    const forced = await fetchHitokotoConfig({ force: true })
    assert.equal(forced.sourceId, 'meigen-ja')

    // 后续普通读取应命中刚写回的新值，不再回源
    const cached = await fetchHitokotoConfig()
    assert.equal(cached.sourceId, 'meigen-ja')
    assert.equal(fresh.calls, 1, 'force 之后的普通读取不该再回源')
  })

  it('请求失败不会把失败状态缓存住', async () => {
    let calls = 0
    apiService.get = (async () => {
      calls += 1
      throw new Error('boom')
    }) as typeof apiService.get

    await assert.rejects(() => fetchHitokotoConfig())
    await assert.rejects(() => fetchHitokotoConfig())

    assert.equal(calls, 2, '失败后下次读取必须重新回源')
  })

  it('配置更新事件带 detail 时直接采纳，不回源', async () => {
    const stub = stubConfig('hitokoto-cn')
    await fetchHitokotoConfig()
    assert.equal(stub.calls, 1)

    windowShim.dispatchEvent(
      new CustomEvent(HITOKOTO_CONFIG_UPDATED_EVENT, {
        detail: { sourceId: 'hitokoto-anime' },
      }),
    )

    const after = await fetchHitokotoConfig()
    assert.equal(after.sourceId, 'hitokoto-anime', '应采纳事件里的新配置')
    assert.equal(stub.calls, 1, '事件带了权威值就不该再回源')
  })

  it('配置更新事件不带 detail 时清空缓存', async () => {
    const stub = stubConfig('hitokoto-cn')
    await fetchHitokotoConfig()
    assert.equal(stub.calls, 1)

    windowShim.dispatchEvent(new CustomEvent(HITOKOTO_CONFIG_UPDATED_EVENT))

    await fetchHitokotoConfig()
    assert.equal(stub.calls, 2, '没有权威值时必须重新回源')
  })
})
