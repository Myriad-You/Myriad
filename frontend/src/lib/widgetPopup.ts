import type {
  WidgetConfig,
  WidgetPopupSize,
  WidgetType,
} from '../components/widgetGridTypes'
import { createStore } from '../utils/store'
import { getTappSubjectSnapshot, subscribeTappSubject } from '../utils/tappSubject'

/**
 * 小组件弹窗。一张卡（实例 id）至多一份弹窗；同一时刻只有一份在前台。
 * 关掉只是收回：内容留在宿主里不卸载，重开接着上次的状态。
 * 只有授权前提消失（换身份、卸载、停用、代码变更、卡片被删）才销毁。
 */
export interface WidgetPopupEntry {
  /** 卡片实例 id；声明了 shareKey 的弹窗用它，几张卡共用一份。 */
  key: string
  widget: WidgetConfig
  widgetType: WidgetType
  size: WidgetPopupSize
  params: unknown
  /** 卡片此刻在视口里的样子；展开从这里出发，收回落到这里。 */
  anchor: () => WidgetPopupAnchor | null
  onConfigChange?: (next: any) => void
  /** 打开次序；收回的超过上限时按它逐出最久没用的。 */
  touchedAt: number
}

/** 卡片看得见的那一面：位置和圆角（px）。收回快落地时用它的样子先换上。 */
export interface WidgetPopupAnchor {
  rect: DOMRect
  radius: number
  face?: HTMLElement
}

export interface WidgetPopupState {
  entries: readonly WidgetPopupEntry[]
  activeKey: string | null
}

/** 收回后留着的上限（不含前台那份）。 */
export const WIDGET_POPUP_STASH_LIMIT = 3

const store = createStore<WidgetPopupState>({ entries: [], activeKey: null })
const armed = createStore(false)

let clock = 0

export const getWidgetPopupState = store.get
export const subscribeWidgetPopupState = store.subscribe
export const isWidgetPopupHostArmed = armed.get
export const subscribeWidgetPopupHost = armed.subscribe

export interface OpenWidgetPopupRequest {
  widget: WidgetConfig
  widgetType: WidgetType
  params?: unknown
  anchor: () => WidgetPopupAnchor | null
  onConfigChange?: (next: any) => void
}

function evictStashed(
  entries: readonly WidgetPopupEntry[],
  activeKey: string | null,
): WidgetPopupEntry[] {
  const stashed = entries
    .filter((entry) => entry.key !== activeKey)
    .toSorted((a, b) => b.touchedAt - a.touchedAt)
  const keep = new Set(
    stashed.slice(0, WIDGET_POPUP_STASH_LIMIT).map((entry) => entry.key),
  )
  return entries.filter(
    (entry) => entry.key === activeKey || keep.has(entry.key),
  )
}

export function openWidgetPopup(request: OpenWidgetPopupRequest): void {
  const popup = request.widgetType.popup
  if (!popup) return
  armed.set(true)
  const key = popup.shareKey ?? request.widget.id
  clock += 1
  const touchedAt = clock
  store.set(({ entries }) => {
    const existing = entries.find((entry) => entry.key === key)
    const next: WidgetPopupEntry = {
      key,
      widget: request.widget,
      widgetType: request.widgetType,
      size: popup.size ?? 'md',
      // 不带参数是「接着上次」；明确给 null 才清掉。
      params: request.params === undefined ? existing?.params ?? null : request.params,
      anchor: request.anchor,
      onConfigChange: request.onConfigChange,
      touchedAt,
    }
    const merged = existing
      ? entries.map((entry) => (entry.key === key ? next : entry))
      : [...entries, next]
    return { entries: evictStashed(merged, key), activeKey: key }
  })
}

/** 收回前台弹窗。给 key 时只收回那一份。 */
export function closeWidgetPopup(key?: string): void {
  store.set((state) => {
    if (!state.activeKey) return state
    if (key && state.activeKey !== key) return state
    return { ...state, activeKey: null }
  })
}

/** 授权前提消失时销毁：连收回留着的一起丢掉。 */
export function dropWidgetPopups(
  predicate: (entry: WidgetPopupEntry) => boolean = () => true,
): void {
  store.set((state) => {
    const entries = state.entries.filter((entry) => !predicate(entry))
    if (entries.length === state.entries.length) return state
    const activeKey = entries.some((entry) => entry.key === state.activeKey)
      ? state.activeKey
      : null
    return { entries, activeKey }
  })
}

/** 卡片配置变了（站长改了实例设置）时，把新配置带进已开的弹窗，不重开。 */
export function syncWidgetPopupConfig(widget: WidgetConfig): void {
  store.set((state) => {
    const index = state.entries.findIndex((entry) => entry.widget.id === widget.id)
    if (index < 0) return state
    const current = state.entries[index]
    if (current.widget === widget) return state
    const entries = state.entries.slice()
    entries[index] = { ...current, widget }
    return { ...state, entries }
  })
}

// 换身份：授权前提没了，全部销毁。在模块里直接订阅，赶在任何弹窗内容用新身份重建之前。
let subjectEpoch = getTappSubjectSnapshot().epoch
subscribeTappSubject(() => {
  const { epoch } = getTappSubjectSnapshot()
  if (epoch === subjectEpoch) return
  subjectEpoch = epoch
  dropWidgetPopups()
})
