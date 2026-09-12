import type { HyaliteAPI, HyaliteOptions } from './vendor/hyalite'

import { createHyalite } from './vendor/hyalite'

const SURFACES = [
  '.glass', '.glass-surface', '.glass-liquid', '.dynamic-island',
  '.secondary-island', '.control-bar-trigger', '.site-footer-content',
  '.surface-dialog', '.tapp-store-frame--wallpaper .as-store__sidebar',
].join(',')
const CHROME = '.control-bar-trigger, .surface-dialog'

export function mountSurfaceLenses(engine: HyaliteAPI = createHyalite()): () => void {
  const root = document.documentElement
  const desktop = matchMedia('(min-width: 768px)')
  const reduced = matchMedia('(prefers-reduced-motion: reduce)')
  const candidates = new Set<HTMLElement>()
  const visible = new Set<HTMLElement>()
  const attached = new Map<HTMLElement, string>()
  const dirty = new Set<HTMLElement>()
  let frame = 0
  let stopped = false

  const detach = (el: HTMLElement) => {
    engine.detach(el)
    el.removeAttribute('data-liquid-lens')
    attached.delete(el)
  }
  const enabled = () => desktop.matches && !reduced.matches
    && !['light', 'exlight'].includes(root.dataset.perfMode ?? '')
    && !document.hidden && engine.supported()

  const flush = () => {
    frame = 0
    if (stopped) return
    const active = enabled()
    if (root.classList.contains('surface-lens') !== active) {
      root.classList.toggle('surface-lens', active)
    }
    const dark = root.classList.contains('dark')
    const globalLiquid = root.dataset.surface === 'liquid'
    const start = performance.now()
    for (const el of dirty) {
      dirty.delete(el)
      const wanted = active && el.isConnected && candidates.has(el)
        && visible.has(el) && el.matches(SURFACES)
        && (globalLiquid || !!el.closest('.glass-liquid'))
        && !el.matches('.agent-panel-overlay-anchor')
        && !el.closest('[data-liquid-lens-skip], .nav-container[data-nav-idle="hidden"], .nav-container.immersive')
      if (!wanted) {
        if (attached.has(el)) detach(el)
      } else {
        const chrome = el.matches(CHROME)
        const key = `${dark}:${chrome}`
        if (attached.get(el) !== key) {
          if (attached.has(el)) detach(el)
          const options: HyaliteOptions = {
            bevel: chrome ? 18 : 16, thickness: 10,
            blur: chrome ? 3 : 2, dispersion: 0, smooth: 1,
            rim: dark ? 0.22 : 0.38, dark, chrome,
            settle: 120, materialize: 0,
          }
          engine.attach(el, options)
          el.setAttribute('data-liquid-lens', chrome ? 'chrome' : 'surface')
          attached.set(el, key)
        } else if (!el.classList.contains('gcp-animating')) {
          engine.refresh(el)
        }
      }
      if (performance.now() - start >= 8) break
    }
    if (dirty.size) schedule()
  }
  const schedule = () => {
    if (!frame && !stopped) frame = requestAnimationFrame(flush)
  }
  const invalidateAll = () => {
    candidates.forEach(el => dirty.add(el))
    schedule()
  }
  const intersection = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      const el = entry.target as HTMLElement
      if (entry.isIntersecting) visible.add(el)
      else visible.delete(el)
      dirty.add(el)
    }
    schedule()
  })
  const discover = (node: Element) => {
    const elements = [
      node,
      ...Iterator.from(node.querySelectorAll(SURFACES)).toArray(),
    ]
    for (const el of elements) {
      if (!(el instanceof HTMLElement) || !el.matches(SURFACES)) continue
      if (!candidates.has(el)) {
        candidates.add(el)
        intersection.observe(el)
      }
      dirty.add(el)
    }
  }
  const bodyObserver = new MutationObserver((mutations) => {
    // Records in one delivery all see the same final DOM. Repeated class
    // changes on a surface need only one subtree discovery/invalidation.
    const attributeTargets = new Set<HTMLElement>()
    const discoveryRoots = new Set<Element>()
    const discoverOnce = (node: Element) => {
      if (discoveryRoots.has(node)) return
      discoveryRoots.add(node)
      discover(node)
    }
    for (const mutation of mutations) {
      if (mutation.type === 'attributes') {
        const target = mutation.target as HTMLElement
        if (attributeTargets.has(target)) continue
        attributeTargets.add(target)
        if (candidates.has(target)) dirty.add(target)
        discoverOnce(target)
        candidates.forEach((el) => {
          if (target.contains(el)) dirty.add(el)
        })
      } else {
        mutation.addedNodes.forEach((node) => {
          if (node instanceof Element) discoverOnce(node)
        })
      }
    }
    for (const el of candidates) {
      if (!el.isConnected || !el.matches(SURFACES)) {
        detach(el)
        intersection.unobserve(el)
        candidates.delete(el)
        visible.delete(el)
        dirty.delete(el)
      }
    }
    schedule()
  })
  const rootObserver = new MutationObserver(invalidateAll)
  const geometryEnded = (event: Event) => {
    if (event instanceof TransitionEvent
      && !/^(width|height|min-width|max-width|min-height|max-height|padding(?:-.+)?|border(?:-.+)?|font-size|line-height|flex-basis|gap|row-gap|column-gap)$/.test(event.propertyName)) { return
}
    if (event instanceof TransitionEvent && /(?:color|style)$/.test(event.propertyName)) return
    const el = event.target
    if (el instanceof HTMLElement && attached.has(el)) {
      dirty.add(el)
      schedule()
    }
  }
  discover(document.body)
  bodyObserver.observe(document.body, {
    subtree: true, childList: true, attributes: true,
    attributeFilter: ['class', 'data-liquid-lens-skip', 'data-nav-idle'],
  })
  rootObserver.observe(root, {
    attributes: true, attributeFilter: ['class', 'data-surface', 'data-perf-mode'],
  })
  desktop.addEventListener('change', invalidateAll)
  reduced.addEventListener('change', invalidateAll)
  document.addEventListener('visibilitychange', invalidateAll)
  document.addEventListener('transitionend', geometryEnded)
  document.addEventListener('animationend', geometryEnded)
  schedule()

  return () => {
    stopped = true
    cancelAnimationFrame(frame)
    bodyObserver.disconnect()
    rootObserver.disconnect()
    intersection.disconnect()
    desktop.removeEventListener('change', invalidateAll)
    reduced.removeEventListener('change', invalidateAll)
    document.removeEventListener('visibilitychange', invalidateAll)
    document.removeEventListener('transitionend', geometryEnded)
    document.removeEventListener('animationend', geometryEnded)
    attached.forEach((_, el) => detach(el))
    engine.dispose()
    candidates.clear()
    visible.clear()
    dirty.clear()
    root.classList.remove('surface-lens')
  }
}
