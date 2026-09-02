/**
 * 思考/在做时给流光两层现场上色。
 *
 * 换谱按车道交接：同一位置旧团先收、新团再进，不会左右对撞。
 * 思考切到在做不算换状态，谱继续走。
 */

import type { RefObject } from 'react'
import { useLayoutEffect } from 'react'
import { applyAuroraPrism, paintAuroraPrism } from './agentAuroraRandom'

/** 同一车道里，旧团开始收之后隔这么久新团再进。 */
export const PRISM_HANDOFF_MS = 340

/** 色团淡出后再卸 DOM，避免思考结束时还占着合成层。 */
export const PRISM_CLEAR_MS = 720

function isPrismStatus(status: string): boolean {
  return status === 'thinking' || status === 'working'
}

function prefersReducedMotion(): boolean {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

function isDark(): boolean {
  return document.documentElement.classList.contains('dark')
}

function paintLayer(el: HTMLElement | null): void {
  if (!el) return
  applyAuroraPrism(el, paintAuroraPrism(Math.random, isDark()))
}

export function useAgentAuroraPrism(
  status: string,
  enabled: boolean,
  layerA: RefObject<HTMLElement | null>,
  layerB: RefObject<HTMLElement | null>,
): void {
  const live = enabled && isPrismStatus(status)

  useLayoutEffect(() => {
    if (!live) return
    const a = layerA.current
    const b = layerB.current
    if (!a || !b) return

    let current = 0
    const layers = [a, b] as const
    paintLayer(layers[0])
    layers[0].removeAttribute('data-active')
    layers[1].removeAttribute('data-active')

    if (prefersReducedMotion()) {
      layers[0].dataset.active = 'true'
      return
    }

    let cancelled = false
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (cancelled) return
        layers[0].dataset.active = 'true'
      })
    })
    let wait = 0
    let handoff = 0
    const schedule = () => {
      wait = window.setTimeout(
        () => {
          const next = 1 - current
          paintLayer(layers[next])
          layers[current].removeAttribute('data-active')
          handoff = window.setTimeout(() => {
            layers[next].dataset.active = 'true'
            current = next
            schedule()
          }, PRISM_HANDOFF_MS)
        },
        7800 + Math.random() * 5200,
      )
    }
    schedule()
    return () => {
      cancelled = true
      window.clearTimeout(wait)
      window.clearTimeout(handoff)
      layers[0].removeAttribute('data-active')
      layers[1].removeAttribute('data-active')
    }
  }, [live, layerA, layerB])

  useLayoutEffect(() => {
    if (live) return undefined
    const a = layerA.current
    const b = layerB.current
    if (!a?.childElementCount && !b?.childElementCount) return undefined
    const timer = window.setTimeout(() => {
      a?.replaceChildren()
      b?.replaceChildren()
    }, PRISM_CLEAR_MS)
    return () => window.clearTimeout(timer)
  }, [live, layerA, layerB])
}
