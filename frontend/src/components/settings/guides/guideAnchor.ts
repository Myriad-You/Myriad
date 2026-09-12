import { prefersReducedMotion, SETTINGS_DURATION_MS } from '../motion'

export const GUIDE_PATH_ATTR = 'data-guide-path'

export function guideAnchorId(path: string): string {
  const cleaned = path.trim().replaceAll(/^\.+|\.+$/g, '')
  if (!cleaned) return ''
  return `cfg-g-${cleaned.replaceAll('.', '-')}`
}

export function guideDomProps(guidePath?: string | null): {
  [GUIDE_PATH_ATTR]?: string
} {
  if (!guidePath?.trim()) return {}
  return { [GUIDE_PATH_ATTR]: guidePath.trim() }
}

export function findGuideElement(path: string): HTMLElement | null {
  if (typeof document === 'undefined' || !path.trim()) return null
  const id = guideAnchorId(path)
  if (id) {
    const byId = document.getElementById(id)
    if (byId) return byId
  }
  const safe = path.replaceAll('\\', '\\\\').replaceAll('"', '\\"')
  return document.querySelector<HTMLElement>(
    `[${GUIDE_PATH_ATTR}="${safe}"]`,
  )
}

export function expandCollapsibleAncestors(el: HTMLElement): boolean {
  let expanded = false
  let node: HTMLElement | null = el
  while (node) {
    if (
      node.classList.contains('setting-group') &&
      node.classList.contains('is-collapsed')
    ) {
      const toggle = node.querySelector<HTMLButtonElement>(
        '.setting-group-header-toggle[aria-expanded="false"]',
      )
      if (toggle) {
        toggle.click()
        expanded = true
      }
    }
    node = node.parentElement
  }
  return expanded
}

const FLASH_CLASS = 'is-guide-flash'
const FLASH_MS = 1600

/** delay scroll if a collapse was opened */
export function scrollToSettingGuide(
  path: string,
  options?: { highlight?: boolean },
): boolean {
  const el = findGuideElement(path)
  if (!el) return false

  const didExpand = expandCollapsibleAncestors(el)
  const highlight = options?.highlight !== false
  const behavior: ScrollBehavior = prefersReducedMotion() ? 'auto' : 'smooth'

  const run = () => {
    el.scrollIntoView({ behavior, block: 'center' })
    if (!highlight) return
    el.classList.remove(FLASH_CLASS)
    void el.offsetWidth // restart CSS animation
    el.classList.add(FLASH_CLASS)
    window.setTimeout(() => {
      el.classList.remove(FLASH_CLASS)
    }, FLASH_MS)
  }

  if (didExpand) {
    window.setTimeout(
      run,
      prefersReducedMotion() ? 0 : SETTINGS_DURATION_MS.base + 40,
    )
  } else {
    requestAnimationFrame(() => {
      requestAnimationFrame(run)
    })
  }
  return true
}

/** after SectionSwitch commit; retry for async sections */
export function scheduleScrollToSettingGuide(
  path: string,
  options?: { attempts?: number; delayMs?: number },
): void {
  const attempts = options?.attempts ?? 8
  const delayMs =
    options?.delayMs ??
    (prefersReducedMotion() ? 0 : SETTINGS_DURATION_MS.slow + 80)

  let left = attempts
  const tryScroll = () => {
    if (scrollToSettingGuide(path)) return
    left -= 1
    if (left <= 0) return
    window.setTimeout(tryScroll, 80)
  }

  window.setTimeout(tryScroll, delayMs)
}
