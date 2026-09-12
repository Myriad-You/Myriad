/** 按 bridge 实例（session token）绑定，同 tappId 多窗互不覆盖。 */

import type { TappBridge } from './TappBridge'
import { isImeComposing } from '../../utils/ime'

export interface HostShortcutBinding {
  tappId: string
  shortcutId: string
  keys: string
  action: string
  scope?: string
  bridge: TappBridge
}

type InternalBinding = HostShortcutBinding & {
  chordParts: string[]
  mainKey: string
  sessionToken: string
}

const bindings = new Map<string, InternalBinding>()
let listenerAttached = false

function bridgeSessionToken(bridge: TappBridge): string {
  try {
    return bridge.getSessionToken() || ''
  } catch {
    return ''
  }
}

function bindingKey(tappId: string, shortcutId: string, sessionToken: string): string {
  return `${tappId}\0${shortcutId}\0${sessionToken}`
}

function normalizeKeys(keys: string): { parts: string[]; mainKey: string } | null {
  const parts = keys
    .split('+')
    .map((p) => p.trim().toLowerCase())
    .filter(Boolean)
  if (parts.length === 0 || parts.length > 4) return null
  const mainKey = parts.at(-1)!
  return { parts, mainKey }
}

function eventMainKey(e: KeyboardEvent): string {
  const k = e.key
  if (k === ' ') return 'space'
  if (k === 'Escape') return 'escape'
  if (k === 'Enter') return 'enter'
  if (k === 'Tab') return 'tab'
  if (k === 'Backspace') return 'backspace'
  if (k === 'Delete') return 'delete'
  if (k === 'ArrowUp') return 'up'
  if (k === 'ArrowDown') return 'down'
  if (k === 'ArrowLeft') return 'left'
  if (k === 'ArrowRight') return 'right'
  if (k === 'Home') return 'home'
  if (k === 'End') return 'end'
  if (k === 'PageUp') return 'pageup'
  if (k === 'PageDown') return 'pagedown'
  if (/^f\d{1,2}$/i.test(k)) return k.toLowerCase()
  if (k.length === 1) return k.toLowerCase()
  return k.toLowerCase()
}

function matchesChord(e: KeyboardEvent, binding: InternalBinding): boolean {
  if (eventMainKey(e) !== binding.mainKey) return false
  const wantCtrl = binding.chordParts.includes('ctrl')
  const wantAlt = binding.chordParts.includes('alt')
  const wantShift = binding.chordParts.includes('shift')
  const wantMeta =
    binding.chordParts.includes('meta') || binding.chordParts.includes('cmd')
  return (
    e.ctrlKey === wantCtrl &&
    e.altKey === wantAlt &&
    e.shiftKey === wantShift &&
    e.metaKey === wantMeta
  )
}

function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  const tag = target.tagName
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true
  if (target.isContentEditable) return true
  return Boolean(target.closest('input, textarea, select, [contenteditable="true"]'))
}

function emitShortcut(binding: InternalBinding): void {
  const payload = {
    shortcutId: binding.shortcutId,
    action: binding.action,
    keys: binding.keys,
    tappId: binding.tappId,
    scope: binding.scope || 'global',
  }

  try {
    binding.bridge.emit('tappEvent', {
      version: 2,
      eventId: `sc_${crypto.randomUUID().replaceAll('-', '')}`,
      topic: 'shortcut:triggered',
      scope: 'instance',
      source: { tappId: binding.tappId, runtimeId: 'host' },
      payload,
      occurredAt: new Date().toISOString(),
    })
    binding.bridge.emit('shortcut:triggered', payload)
  } catch (err) {
    console.warn('[HostShortcut] emit failed:', err)
  }
}

function isLiveBridge(bridge: TappBridge): boolean {
  try {
    if (typeof bridge.isDestroyed === 'function' && bridge.isDestroyed()) {
      return false
    }
    const token = bridgeSessionToken(bridge)
    if (!token) return false
    if (typeof bridge.isSurfaceActive === 'function' && !bridge.isSurfaceActive()) {
      return false
    }
    return true
  } catch {
    return false
  }
}

function onKeyDown(e: KeyboardEvent) {
  if (e.defaultPrevented || e.repeat) return
  if (isImeComposing(e)) return
  if (isTypingTarget(e.target)) return
  if (bindings.size === 0) return

  // 空 token / 已销毁 / 非活动表面跳过。孤儿匹配不 preventDefault。
  let matched = false
  for (const [key, binding] of Iterator.from(bindings.entries()).toArray()) {
    if (!matchesChord(e, binding)) continue
    if (!isLiveBridge(binding.bridge)) {
      if (
        !bridgeSessionToken(binding.bridge) ||
        (typeof binding.bridge.isDestroyed === 'function' &&
          binding.bridge.isDestroyed())
      ) {
        bindings.delete(key)
      }
      continue
    }
    if (!matched) {
      e.preventDefault()
      e.stopPropagation()
      matched = true
    }
    emitShortcut(binding)
  }
  if (!matched) maybeDetachListener()
}

function ensureListener() {
  if (listenerAttached || typeof window === 'undefined') return
  window.addEventListener('keydown', onKeyDown, true)
  listenerAttached = true
}

function maybeDetachListener() {
  if (!listenerAttached || bindings.size > 0) return
  window.removeEventListener('keydown', onKeyDown, true)
  listenerAttached = false
}

export function hostBindShortcut(binding: HostShortcutBinding): void {
  const normalized = normalizeKeys(binding.keys)
  if (!normalized) {
    console.warn('[HostShortcut] invalid keys:', binding.keys)
    return
  }
  try {
    if (
      typeof binding.bridge.isDestroyed === 'function' &&
      binding.bridge.isDestroyed()
    ) {
      console.warn('[HostShortcut] bridge destroyed; skip bind')
      return
    }
  } catch {
    return
  }
  const sessionToken = bridgeSessionToken(binding.bridge)
  // 空 token 视为死，不绑定孤儿和弦。
  if (!sessionToken) {
    console.warn('[HostShortcut] bridge has no session token; skip bind')
    return
  }
  bindings.set(bindingKey(binding.tappId, binding.shortcutId, sessionToken), {
    ...binding,
    chordParts: normalized.parts,
    mainKey: normalized.mainKey,
    sessionToken,
  })
  ensureListener()
}

/** 按 bridge 解绑，同 app 其他窗保留绑定。 */
export function hostUnbindShortcut(
  tappId: string,
  shortcutId: string,
  bridge?: TappBridge,
): void {
  if (bridge) {
    const sessionToken = bridgeSessionToken(bridge)
    bindings.delete(bindingKey(tappId, shortcutId, sessionToken))
  } else {
    for (const key of Iterator.from(bindings.keys()).toArray()) {
      if (key.startsWith(`${tappId}\0${shortcutId}\0`)) bindings.delete(key)
    }
  }
  maybeDetachListener()
}

/** 按 bridge 拆除（沙箱销毁）。不要用按 tapp 全解绑，以免误伤其他窗。 */
export function hostUnbindAllForBridge(bridge: TappBridge): void {
  const sessionToken = bridgeSessionToken(bridge)
  if (!sessionToken) {
    for (const [key, binding] of Iterator.from(bindings.entries()).toArray()) {
      if (binding.bridge === bridge) bindings.delete(key)
    }
  } else {
    for (const [key, binding] of Iterator.from(bindings.entries()).toArray()) {
      if (binding.sessionToken === sessionToken || binding.bridge === bridge) {
        bindings.delete(key)
      }
    }
  }
  maybeDetachListener()
}

/** 按 Tapp 跨桥拆除。沙箱销毁应走 hostUnbindAllForBridge。 */
export function hostUnbindAllForTapp(tappId: string): void {
  for (const key of Iterator.from(bindings.keys()).toArray()) {
    if (key.startsWith(`${tappId}\0`)) bindings.delete(key)
  }
  maybeDetachListener()
}
