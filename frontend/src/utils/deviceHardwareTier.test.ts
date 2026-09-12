import type { HardwareSignals } from './deviceHardwareTier'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import {
  detectAppleSilicon,
  detectAppleSiliconAsync,
  evaluateHighHardware,

  resetAppleSiliconCache,
} from './deviceHardwareTier'

function sig(partial: Partial<HardwareSignals>): HardwareSignals {
  return {
    os: 'unknown',
    cores: null,
    memoryGiB: null,
    iosMajor: null,
    appleSilicon: null,
    ...partial,
  }
}

describe('evaluateHighHardware', () => {
  it('android requires 8GB bucket and 8 cores', () => {
    assert.equal(
      evaluateHighHardware(sig({ os: 'android', memoryGiB: 8, cores: 8 }))
        .highHardware,
      true,
    )
    assert.equal(
      evaluateHighHardware(sig({ os: 'android', memoryGiB: 4, cores: 8 }))
        .highHardware,
      false,
    )
    assert.equal(
      evaluateHighHardware(sig({ os: 'android', memoryGiB: 8, cores: 6 }))
        .highHardware,
      false,
    )
  })

  it('ios high when major >= 18', () => {
    assert.equal(
      evaluateHighHardware(sig({ os: 'ios', iosMajor: 18 })).highHardware,
      true,
    )
    assert.equal(
      evaluateHighHardware(sig({ os: 'ios', iosMajor: 26 })).highHardware,
      true,
    )
    assert.equal(
      evaluateHighHardware(sig({ os: 'ios', iosMajor: 17 })).highHardware,
      false,
    )
  })

  it('macos M-series high, Intel low', () => {
    assert.equal(
      evaluateHighHardware(sig({ os: 'macos', appleSilicon: true }))
        .highHardware,
      true,
    )
    assert.equal(
      evaluateHighHardware(sig({ os: 'macos', appleSilicon: false }))
        .highHardware,
      false,
    )
    assert.equal(
      evaluateHighHardware(sig({ os: 'macos', appleSilicon: null }))
        .highHardware,
      false,
    )
  })

  it('windows/linux need ~12GB bucket (>=8) and 6+ cores', () => {
    assert.equal(
      evaluateHighHardware(sig({ os: 'windows', memoryGiB: 8, cores: 6 }))
        .highHardware,
      true,
    )
    assert.equal(
      evaluateHighHardware(sig({ os: 'linux', memoryGiB: 8, cores: 4 }))
        .highHardware,
      false,
    )
    assert.equal(
      evaluateHighHardware(sig({ os: 'windows', memoryGiB: 4, cores: 8 }))
        .highHardware,
      false,
    )
  })
})

describe('detectAppleSilicon WebGL budget', () => {
  const g = globalThis as typeof globalThis & {
    document?: unknown
    WebGLRenderingContext?: unknown
    navigator?: unknown
  }
  const prevDoc = g.document
  const prevWebGl = g.WebGLRenderingContext

  afterEach(() => {
    resetAppleSiliconCache()
    if (prevDoc === undefined) delete g.document
    else g.document = prevDoc
    if (prevWebGl === undefined) delete g.WebGLRenderingContext
    else g.WebGLRenderingContext = prevWebGl
  })

  it('detects arm64 from UA without opening WebGL', () => {
    let getContextCount = 0
    g.WebGLRenderingContext = function WebGLRenderingContext() {}
    g.document = {
      createElement() {
        return {
          getContext() {
            getContextCount++
            return null
          },
          width: 0,
          height: 0,
        }
      },
    }
    const nav = {
      userAgent:
        'Mozilla/5.0 (Macintosh; ARM64 Mac OS X 14_0) AppleWebKit/605.1.15',
    } as Navigator
    assert.equal(detectAppleSilicon(nav), true)
    assert.equal(getContextCount, 0)
  })

  it('Chromium high-entropy path skips sync WebGL', () => {
    let getContextCount = 0
    g.WebGLRenderingContext = function WebGLRenderingContext() {}
    g.document = {
      createElement() {
        return {
          getContext() {
            getContextCount++
            return null
          },
          width: 0,
          height: 0,
        }
      },
    }
    const nav = {
      userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)',
      userAgentData: {
        getHighEntropyValues: async () => ({ architecture: 'arm', platform: 'macOS' }),
      },
    } as unknown as Navigator
    assert.equal(detectAppleSilicon(nav), null)
    assert.equal(getContextCount, 0)
  })

  it('probes WebGL at most once and calls loseContext', () => {
    let getContextCount = 0
    let loseCount = 0
    const gl = {
      getExtension(name: string) {
        if (name === 'WEBGL_lose_context') {
          return { loseContext: () => {
            loseCount++
          } }
        }
        if (name === 'WEBGL_debug_renderer_info') {
          return { UNMASKED_RENDERER_WEBGL: 0x9246 }
        }
        return null
      },
      getParameter() {
        return 'Apple M2 Pro'
      },
    }
    g.WebGLRenderingContext = function WebGLRenderingContext() {}
    g.document = {
      createElement(tag: string) {
        assert.equal(tag, 'canvas')
        return {
          getContext(type: string) {
            getContextCount++
            return type === 'webgl' ? gl : null
          },
          width: 0,
          height: 0,
        }
      },
    }
    const nav = {
      userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)',
    } as Navigator

    assert.equal(detectAppleSilicon(nav), true)
    assert.equal(detectAppleSilicon(nav), true)
    assert.equal(detectAppleSilicon(nav), true)
    assert.equal(getContextCount, 1, 'getContext must run once')
    assert.equal(loseCount, 1, 'loseContext must run once after probe')
  })

  it('async architecture sets cache without WebGL', async () => {
    let getContextCount = 0
    g.WebGLRenderingContext = function WebGLRenderingContext() {}
    g.document = {
      createElement() {
        return {
          getContext() {
            getContextCount++
            return null
          },
          width: 0,
          height: 0,
        }
      },
    }
    const nav = {
      userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)',
      userAgentData: {
        getHighEntropyValues: async () => ({
          architecture: 'x86',
          platform: 'macOS',
        }),
      },
    } as unknown as Navigator

    assert.equal(await detectAppleSiliconAsync(nav), false)
    assert.equal(detectAppleSilicon(nav), false)
    assert.equal(getContextCount, 0)
  })
})
