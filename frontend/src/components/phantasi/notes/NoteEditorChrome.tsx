import type { CSSProperties, FocusEvent, KeyboardEvent, ReactNode, RefObject } from 'react'
import type { WidgetType } from '../../widgetGridTypes'
/**
 * 写作器的外壳：薄顶栏、标题下的一行元信息、选区浮动条、底栏、发布抽屉。
 * 控件全是 NoteControls 里自己的，不借设置页；这里不碰 phantasiApi。
 */

import type { NoteCollabPeer } from './noteCollab'
import type { NoteImageValues } from './NoteImageFields'
import type { PopoverCoords } from './notePopover'
import type { NoteSchedulePresetKey } from './noteSchedulePresets'
import type { SelectionAnchor, VisualBlockKind } from './noteSelection'

import {
  LuCheck as Check,
  LuClock as Clock,
  LuCopy as Copy,
  LuExternalLink as ExternalLink,
  LuImage as Image,
  LuPlus as Plus,
  LuReplace as Replace,
  LuSend as Send,
  LuSlidersHorizontal as SlidersHorizontal,
  LuTextAlignCenter as TextAlignCenter,
  LuTextAlignEnd as TextAlignEnd,
  LuTextAlignStart as TextAlignStart,
  LuTrash2 as Trash2,
  LuUnlink as Unlink,
  LuX as X,
} from '@lib/icons'
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useI18n } from '../../../contexts/I18nContext'
import { phantasiMotionQuiet } from '../../../hooks/animation/pages/phantasiMotion'
import { noteEditorStatus } from './noteBoard'
import { normalizeNoteCategory, noteCategoryLabel } from './noteCategory'
import {
  NoteButton,
  NoteChip,
  NoteDateInput,
  NoteField,
  NoteSection,
  NoteSwitch,
} from './NoteControls'
import { NoteImageFields } from './NoteImageFields'
import { placePopover } from './notePopover'
import { noteRelativeUntil, noteSchedulePresets } from './noteSchedulePresets'
import { placeBubble, placeGutter } from './noteSelection'
import { NoteWidgetPage } from './NoteWidgetPicker'

export type NoteEditorPane = 'write' | 'visual' | 'preview'
type NoteEditorDocStatus = 'draft' | 'scheduled' | 'published'

export interface NoteEditorTool {
  key: string
  icon: ReactNode
  label: string
  run: () => void
  /** 有这个就先在菜单里要一个值（比如图片地址），回车再 `runWith`。 */
  prompt?: string
  runWith?: (value: string) => void
  /** 点了不关菜单，翻到同一浮层的二级页（`run` 先跑一次，用来记住选区）。 */
  page?: NoteInsertPage
}

export type NoteInsertPage = 'widgets'

/** 选中文字才用得上的放浮动条，其余是块级插入。 */
const MARK_TOOL_KEYS = [
  'bold',
  'italic',
  'strike',
  'inline-code',
  'inline-math',
  'link',
  'h2',
  'h3',
  'quote',
]

export function splitNoteTools(tools: NoteEditorTool[]): {
  marks: NoteEditorTool[]
  inserts: NoteEditorTool[]
} {
  const marks = MARK_TOOL_KEYS.flatMap((key) =>
    tools.filter((tool) => tool.key === key),
  )
  const inserts = tools.filter((tool) => !MARK_TOOL_KEYS.includes(tool.key))
  return { marks, inserts }
}

const DRAWER_LEAVE_MS = 220
const MENU_LEAVE_MS = 140

function usePresence(
  open: boolean,
  leaveMs: number,
): { shown: boolean; leaving: boolean } {
  const [shown, setShown] = useState(open)
  const [leaving, setLeaving] = useState(false)
  useEffect(() => {
    if (open) {
      setShown(true)
      setLeaving(false)
      return
    }
    if (!shown) return
    if (phantasiMotionQuiet()) {
      setShown(false)
      setLeaving(false)
      return
    }
    setLeaving(true)
    const timer = window.setTimeout(() => {
      setShown(false)
      setLeaving(false)
    }, leaveMs)
    return () => window.clearTimeout(timer)
  }, [leaveMs, open, shown])
  return { shown, leaving }
}

function useDrawerPresence(open: boolean): { shown: boolean; leaving: boolean } {
  return usePresence(open, DRAWER_LEAVE_MS)
}

function peerInitial(peer: NoteCollabPeer): string {
  const name = peer.name?.trim()
  return name ? [...name][0]!.toUpperCase() : '·'
}

/** 协作者各领一个固定色，不跟壁纸走。 */
const PEER_HUES = [14, 152, 210, 268, 328, 44]

export function peerHue(peerId: string): number {
  let hash = 0
  for (const ch of peerId) hash = (hash * 31 + ch.codePointAt(0)!) >>> 0
  return PEER_HUES[hash % PEER_HUES.length]!
}

/* ------------------------------------------------------------------ */

interface NoteTopBarProps {
  docStatus: NoteEditorDocStatus
  lastError: string | null
  scheduledAt: number | null
  cloudHint: boolean
  peers: NoteCollabPeer[]
  collabConnection: 'connecting' | 'connected' | 'reconnecting'
  saving: boolean
  loading: boolean
  onPublish: () => void
  articleHref?: string | null
  settingsOpen: boolean
  onToggleSettings: () => void
  /** 不给就不放关闭按钮（窗口自己有）。 */
  onClose?: () => void
}

export function NoteTopBar({
  docStatus,
  lastError,
  scheduledAt,
  cloudHint,
  peers,
  collabConnection,
  saving,
  loading,
  onPublish,
  articleHref,
  settingsOpen,
  onToggleSettings,
  onClose,
}: NoteTopBarProps) {
  const { t, format, locale } = useI18n()
  const [now, setNow] = useState(Date.now)
  useEffect(() => {
    if (!peers.length) return
    setNow(Date.now())
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [peers])
  const peerLabel = (peer: NoteCollabPeer) => format(
    peer.lastEditAt != null && now - peer.lastEditAt < 5000 ? t.phantasi.noteCollabTyping : t.phantasi.noteCollabOnline,
    { name: peer.name || t.phantasi.noteHistoryUnknown },
  )
  const statusLabel = noteEditorStatus(
    { lastError, status: docStatus, scheduledAt, savedHint: cloudHint },
    {
      failed: t.phantasi.noteScheduleFailed,
      scheduled: t.phantasi.noteStatusScheduled,
      published: t.phantasi.noteStatusPublished,
      draft: t.phantasi.noteStatusDraft,
      saved: t.phantasi.noteDraftSaved,
    },
    locale,
  )
  return (
    <header className="phantasi-note__top">
      <div className="phantasi-note__top-row">
        <div className="phantasi-note__top-left">
          {onClose ? (
            <NoteButton
              variant="quiet"
              icon={<X />}
              onClick={onClose}
              title={t.phantasi.close}
              aria-label={t.phantasi.close}
            />
          ) : null}
          <span
            className={`phantasi-note__state${lastError ? ' is-failed' : ''}`}
            role="status"
          >
            {statusLabel}
          </span>
          {collabConnection !== 'connected' ? <span className="phantasi-note__collab-state" role="status">
            {collabConnection === 'connecting' ? t.phantasi.noteCollabConnecting : t.phantasi.noteCollabReconnecting}
          </span> : null}
          {peers.length ? (
            <div className="phantasi-note__peers" aria-live="polite">
              {peers.map((peer) => (
                <span key={peer.peerId} className="phantasi-note__collaborator" title={peerLabel(peer)}>
                  <span className="phantasi-note__peer" style={{ '--peer-hue': peerHue(peer.peerId) } as CSSProperties} aria-hidden="true">{peerInitial(peer)}</span>
                  <span>{peerLabel(peer)}</span>
                </span>
              ))}
            </div>
          ) : null}
        </div>
        <div className="phantasi-note__top-right">
          {articleHref ? (
            <a className="note-btn note-btn--outline note-btn--lg" href={articleHref} target="_blank" rel="noopener noreferrer">
              <span className="note-btn__label">{t.studio.viewArticle}</span>
            </a>
          ) : null}
          <NoteButton
            variant="solid"
            size="lg"
            loading={saving}
            disabled={loading}
            onClick={onPublish}
          >
            {t.phantasi.notePublish}
          </NoteButton>
          <NoteButton
            variant="quiet"
            icon={<SlidersHorizontal />}
            active={settingsOpen}
            aria-pressed={settingsOpen}
            aria-expanded={settingsOpen}
            aria-label={t.phantasi.notePublishSettings}
            title={t.phantasi.notePublishSettings}
            onClick={onToggleSettings}
          />
        </div>
      </div>
    </header>
  )
}

/* ------------------------------------------------------------------ */

interface NoteBylineProps {
  authorLine: string | null
  topic: string | null
  publishedAt: number | null
  scheduledAt: number | null
  docStatus: NoteEditorDocStatus
  onOpenSettings: () => void
}

/** 标题下面一行淡字：联合作者 · 分类 · 时间。点开发布设置。 */
export function NoteByline({
  authorLine,
  topic,
  publishedAt,
  scheduledAt,
  docStatus,
  onOpenSettings,
}: NoteBylineProps) {
  const { t, locale } = useI18n()
  const topicLabel = topic
    ? noteCategoryLabel(topic, t.phantasi)
    : t.phantasi.noteTopicNone
  const when = docStatus === 'scheduled' ? scheduledAt : publishedAt
  const whenLabel =
    when != null && Number.isFinite(when)
      ? new Date(when).toLocaleString(locale, {
          dateStyle: 'medium',
          timeStyle: 'short',
        })
      : null
  return (
    <button
      type="button"
      className="phantasi-note__byline"
      onClick={onOpenSettings}
      title={t.phantasi.notePublishSettings}
    >
      {authorLine ? (
        <>
          <span>{authorLine}</span>
          <span aria-hidden="true">·</span>
        </>
      ) : null}
      <span>{topicLabel}</span>
      {whenLabel ? (
        <>
          <span aria-hidden="true">·</span>
          <span>
            {docStatus === 'scheduled' ? `${t.phantasi.noteStatusScheduled} ` : ''}
            {whenLabel}
          </span>
        </>
      ) : null}
    </button>
  )
}

/* ------------------------------------------------------------------ */

interface NoteBubbleProps {
  anchor: SelectionAnchor | null
  tools: NoteEditorTool[]
  /** 选区上已经有的记号，对应按钮亮起。 */
  active: ReadonlySet<string>
  containerRef: RefObject<HTMLElement | null>
  /** 链接地址栏打开时，浮动条改成一行输入框。 */
  linkOpen: boolean
  linkInitial: string
  onLinkOpenChange: (open: boolean) => void
  /** `null` 表示移除链接。 */
  onLink: (url: string | null) => void
}

/**
 * 选中文字才出现，压在选区上方。深底白字，和 Medium 一样不抢正文。
 * 选区没了、编辑器失焦、地址栏失焦：都收掉，不留残影。
 */
export function NoteBubble({
  anchor,
  tools,
  active,
  containerRef,
  linkOpen,
  linkInitial,
  onLinkOpenChange,
  onLink,
}: NoteBubbleProps) {
  const { t } = useI18n()
  const ref = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const [placement, setPlacement] = useState<{ top: number; left: number } | null>(
    null,
  )
  const [url, setUrl] = useState('')

  useLayoutEffect(() => {
    const bubble = ref.current
    const container = containerRef.current
    if (!anchor || !bubble || !container) {
      setPlacement(null)
      return
    }
    setPlacement(
      placeBubble(
        anchor,
        { width: bubble.offsetWidth, height: bubble.offsetHeight },
        { width: container.clientWidth, scrollTop: container.scrollTop },
        8,
        8,
        window.matchMedia('(pointer: coarse)').matches,
      ),
    )
  }, [anchor, containerRef, linkOpen])

  useEffect(() => {
    if (!linkOpen) {
      setUrl('')
      return
    }
    setUrl(linkInitial)
    inputRef.current?.focus()
    inputRef.current?.select()
  }, [linkOpen, linkInitial])

  // 地址栏没了锚点（选区被别的操作清掉）也要跟着关，不然 hold 会把它钉在屏幕上。
  useEffect(() => {
    if (linkOpen && !anchor) onLinkOpenChange(false)
  }, [anchor, linkOpen, onLinkOpenChange])

  if (!anchor || tools.length === 0) return null
  return (
    <div
      ref={ref}
      className={`phantasi-note__bubble${placement ? ' is-placed' : ''}${linkOpen ? ' is-link' : ''}`}
      style={
        placement
          ? { top: `${placement.top}px`, left: `${placement.left}px` }
          : undefined
      }
      role="toolbar"
    >
      {linkOpen ? (
        <form
          className="phantasi-note__bubble-link"
          onSubmit={(event) => {
            event.preventDefault()
            const value = url.trim()
            onLink(value || null)
          }}
          onBlur={(event) => {
            // 焦点离开整个地址栏（不是在输入框和「移除」之间挪）就关掉。
            const next = event.relatedTarget as Node | null
            if (next && ref.current?.contains(next)) return
            onLinkOpenChange(false)
          }}
        >
          <input
            ref={inputRef}
            className="phantasi-note__bubble-input"
            type="url"
            inputMode="url"
            value={url}
            placeholder={t.phantasi.noteLinkPlaceholder}
            aria-label={t.phantasi.noteToolLink}
            onChange={(event) => setUrl(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Escape') {
                event.preventDefault()
                event.stopPropagation()
                onLinkOpenChange(false)
              }
            }}
          />
          {linkInitial ? (
            <button
              type="button"
              className="phantasi-note__bubble-btn"
              title={t.phantasi.noteLinkRemove}
              aria-label={t.phantasi.noteLinkRemove}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => onLink(null)}
            >
              <Unlink />
            </button>
          ) : null}
        </form>
      ) : (
        tools.map((tool) => {
          const on = active.has(tool.key)
          const text = /^h\d$/.test(tool.key)
          return (
            <button
              key={tool.key}
              type="button"
              className={`phantasi-note__bubble-btn${text ? ' phantasi-note__bubble-btn--text' : ''}${on ? ' is-active' : ''}`}
              title={tool.label}
              aria-label={tool.label}
              aria-pressed={on}
              onMouseDown={(event) => event.preventDefault()}
              onClick={tool.key === 'link' ? () => onLinkOpenChange(true) : tool.run}
            >
              {tool.icon}
            </button>
          )
        })
      )}
    </div>
  )
}

/* ------------------------------------------------------------------ */

export type TableAlign = 'left' | 'center' | 'right' | null

interface NoteBlockBarProps {
  block: { kind: VisualBlockKind | 'img'; anchor: SelectionAnchor } | null
  codeLang: string
  onCodeLangChange: (lang: string) => void
  columnAlign: TableAlign
  onTableAlign: (align: TableAlign) => void
  onTableAddRow: () => void
  onTableAddColumn: () => void
  onTableRemoveRow: () => void
  onTableRemoveColumn: () => void
  onTableRemove: () => void
  imageAlt: string
  imageSrc: string
  imageHref: string
  onImageApply: (values: NoteImageValues) => void
  onImageReplace: () => void
  onImageRemove: () => void
  onColumnsAdd: () => void
  onColumnsRemoveCol: () => void
  onColumnsRemove: () => void
  widgetSize: string
  widgetSizes: string[]
  onWidgetSize: (size: string) => void
  canConfigure?: boolean
  widgetConfigOpen?: boolean
  onWidgetConfig?: () => void
  onWidgetRemove: () => void
  onFocusChange: (focused: boolean) => void
  barRef?: RefObject<HTMLDivElement | null>
}

function BlockBarButton({
  label,
  icon,
  danger,
  active,
  onClick,
}: {
  label: string
  icon?: ReactNode
  danger?: boolean
  active?: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      className={`phantasi-note__blockbar-btn${danger ? ' is-danger' : ''}${active ? ' is-active' : ''}${icon ? ' has-icon' : ''}`}
      title={label}
      aria-label={label}
      aria-pressed={active}
      onMouseDown={(event) => event.preventDefault()}
      onClick={onClick}
    >
      {icon ?? label}
    </button>
  )
}

function BlockBarField({
  label,
  value,
  placeholder,
  mono,
  onChange,
  onFocusChange,
}: {
  label: string
  value: string
  placeholder?: string
  mono?: boolean
  onChange: (value: string) => void
  onFocusChange: (focused: boolean) => void
}) {
  return (
    <label className="phantasi-note__blockbar-field">
      <span>{label}</span>
      <input
        className={`phantasi-note__blockbar-input${mono ? ' is-mono' : ''}`}
        value={value}
        placeholder={placeholder}
        spellCheck={false}
        onFocus={() => onFocusChange(true)}
        onBlur={() => onFocusChange(false)}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === 'Enter' || event.key === 'Escape') {
            event.preventDefault()
            event.stopPropagation()
            ;(event.currentTarget as HTMLInputElement).blur()
          }
        }}
      />
    </label>
  )
}

/**
 * 光标在表格 / 代码块 / 分栏里，或者点中了一张图或小组件，块右上角的一小条。
 */
export function NoteBlockBar({
  block,
  codeLang,
  onCodeLangChange,
  columnAlign,
  onTableAlign,
  onTableAddRow,
  onTableAddColumn,
  onTableRemoveRow,
  onTableRemoveColumn,
  onTableRemove,
  imageAlt,
  imageSrc,
  imageHref,
  onImageApply,
  onImageReplace,
  onImageRemove,
  onColumnsAdd,
  onColumnsRemoveCol,
  onColumnsRemove,
  widgetSize,
  widgetSizes,
  onWidgetSize,
  canConfigure,
  widgetConfigOpen,
  onWidgetConfig,
  onWidgetRemove,
  onFocusChange,
  barRef,
}: NoteBlockBarProps) {
  const { t } = useI18n()
  const [imagePlacement, setImagePlacement] = useState<{ top: number; left: number } | null>(null)
  useLayoutEffect(() => {
    if (block?.kind !== 'img') return
    const bar = barRef?.current
    const container = bar?.parentElement
    if (!bar || !container) return
    const place = () => {
      const left = Math.max(8, Math.min(block.anchor.left, container.clientWidth - bar.offsetWidth - 8))
      const preferred = block.anchor.top - bar.offsetHeight - 8
      const top = Math.max(container.scrollTop + 8, Math.min(preferred, container.scrollTop + container.clientHeight - bar.offsetHeight - 8))
      setImagePlacement(current => current?.top === top && current.left === left ? current : { top, left })
    }
    place()
    const observer = new ResizeObserver(place)
    observer.observe(bar)
    observer.observe(container)
    container.addEventListener('scroll', place, { passive: true })
    return () => { observer.disconnect(); container.removeEventListener('scroll', place) }
  }, [block?.kind, block?.anchor.top, block?.anchor.left, barRef])
  if (!block) return null
  const { anchor, kind } = block
  return (
    <div
      ref={barRef}
      key={kind}
      className={`phantasi-note__blockbar${kind === 'img' ? ' phantasi-note__blockbar--image' : ''}`}
      style={{
        top: `${kind === 'img' ? imagePlacement?.top ?? anchor.top : anchor.top - 8}px`,
        left: `${kind === 'img' ? imagePlacement?.left ?? anchor.left : anchor.left + anchor.width}px`,
      }}
      role="toolbar"
    >
      {kind === 'table' ? (
        <>
          <BlockBarButton
            label={t.phantasi.noteTableAlignLeft}
            icon={<TextAlignStart />}
            active={columnAlign === 'left'}
            onClick={() => onTableAlign(columnAlign === 'left' ? null : 'left')}
          />
          <BlockBarButton
            label={t.phantasi.noteTableAlignCenter}
            icon={<TextAlignCenter />}
            active={columnAlign === 'center'}
            onClick={() => onTableAlign(columnAlign === 'center' ? null : 'center')}
          />
          <BlockBarButton
            label={t.phantasi.noteTableAlignRight}
            icon={<TextAlignEnd />}
            active={columnAlign === 'right'}
            onClick={() => onTableAlign(columnAlign === 'right' ? null : 'right')}
          />
          <span className="phantasi-note__blockbar-sep" aria-hidden="true" />
          <BlockBarButton label={t.phantasi.noteTableAddRow} onClick={onTableAddRow} />
          <BlockBarButton label={t.phantasi.noteTableAddColumn} onClick={onTableAddColumn} />
          <BlockBarButton label={t.phantasi.noteTableDeleteRow} onClick={onTableRemoveRow} />
          <BlockBarButton
            label={t.phantasi.noteTableDeleteColumn}
            onClick={onTableRemoveColumn}
          />
          <span className="phantasi-note__blockbar-sep" aria-hidden="true" />
          <BlockBarButton
            label={t.phantasi.noteTableRemove}
            icon={<Trash2 />}
            danger
            onClick={onTableRemove}
          />
        </>
      ) : kind === 'img' ? (
        <>
          <NoteImageFields values={{ alt: imageAlt, src: imageSrc, href: imageHref }} onApply={onImageApply} onFocusChange={onFocusChange} />
          <BlockBarButton
            label={t.phantasi.noteImageReplace}
            icon={<Replace />}
            onClick={onImageReplace}
          />
          <BlockBarButton
            label={t.phantasi.noteImageRemove}
            icon={<Trash2 />}
            danger
            onClick={onImageRemove}
          />
        </>
      ) : kind === 'columns' ? (
        <>
          <BlockBarButton label={t.phantasi.noteColumnsAdd} onClick={onColumnsAdd} />
          <BlockBarButton label={t.phantasi.noteColumnsRemove} onClick={onColumnsRemoveCol} />
          <span className="phantasi-note__blockbar-sep" aria-hidden="true" />
          <BlockBarButton
            label={t.phantasi.noteColumnsDelete}
            icon={<Trash2 />}
            danger
            onClick={onColumnsRemove}
          />
        </>
      ) : kind === 'widget' ? (
        <>
          {widgetSizes.map((size) => (
            <BlockBarButton
              key={size}
              label={`${t.phantasi.noteWidgetSize} ${size}`}
              active={widgetSize === size}
              onClick={() => onWidgetSize(size)}
            />
          ))}
          {canConfigure && onWidgetConfig ? (
            <BlockBarButton
              label={t.phantasi.noteWidgetConfig}
              icon={<SlidersHorizontal />}
              active={widgetConfigOpen}
              onClick={onWidgetConfig}
            />
          ) : null}
          <span className="phantasi-note__blockbar-sep" aria-hidden="true" />
          <BlockBarButton
            label={t.phantasi.noteWidgetRemove}
            icon={<Trash2 />}
            danger
            onClick={onWidgetRemove}
          />
        </>
      ) : (
        <BlockBarField
          label={t.phantasi.noteCodeLang}
          value={codeLang}
          placeholder="rust"
          mono
          onChange={onCodeLangChange}
          onFocusChange={onFocusChange}
        />
      )}
    </div>
  )
}

/* ------------------------------------------------------------------ */

export type NoteInsertMenuState = null | 'pointer' | 'keys'

interface NoteGutterProps {
  /** 光标停在空行时的位置；null 就不画。 */
  caretLine: SelectionAnchor | null
  menu: NoteInsertMenuState
  onMenuChange: (menu: NoteInsertMenuState) => void
  items: NoteEditorTool[]
  busy: boolean
  /** 二级页「小组件」的目录和落点。 */
  widgets: WidgetType[]
  onPickWidget: (type: string, size: string) => void
}

const GUTTER_SIZE = 36

type NoteInsertGroup = 'media' | 'text' | 'list' | 'block'

const INSERT_GROUP_ORDER: NoteInsertGroup[] = ['media', 'text', 'list', 'block']

/** 插入菜单按用途分组成双列卡片；没登记的新工具落进最后一组。 */
const INSERT_GROUP_OF: Record<string, NoteInsertGroup> = {
  image: 'media',
  'image-url': 'media',
  widget: 'media',
  columns: 'media',
  'h1-block': 'text',
  'h2-block': 'text',
  'h3-block': 'text',
  footnote: 'text',
  list: 'list',
  ol: 'list',
  task: 'list',
  table: 'list',
}

/** 卡片上顺手教一下：可视层行首敲这些也能得到同一个块（见 noteInputRules）。 */
const INSERT_SHORTCUT: Record<string, string> = {
  'h1-block': '#',
  'h2-block': '##',
  'h3-block': '###',
  list: '-',
  ol: '1.',
  task: '[ ]',
  code: '```',
  'display-math': '$$',
  divider: '---',
}

type InsertDescKey =
  | 'noteInsertDescImage'
  | 'noteInsertDescImageUrl'
  | 'noteInsertDescWidget'
  | 'noteInsertDescColumns'
  | 'noteInsertDescH1'
  | 'noteInsertDescH2'
  | 'noteInsertDescH3'
  | 'noteInsertDescFootnote'
  | 'noteInsertDescList'
  | 'noteInsertDescOl'
  | 'noteInsertDescTask'
  | 'noteInsertDescTable'
  | 'noteInsertDescCode'
  | 'noteInsertDescMath'
  | 'noteInsertDescDivider'

const INSERT_DESC: Record<string, InsertDescKey> = {
  image: 'noteInsertDescImage',
  'image-url': 'noteInsertDescImageUrl',
  widget: 'noteInsertDescWidget',
  columns: 'noteInsertDescColumns',
  'h1-block': 'noteInsertDescH1',
  'h2-block': 'noteInsertDescH2',
  'h3-block': 'noteInsertDescH3',
  footnote: 'noteInsertDescFootnote',
  list: 'noteInsertDescList',
  ol: 'noteInsertDescOl',
  task: 'noteInsertDescTask',
  table: 'noteInsertDescTable',
  code: 'noteInsertDescCode',
  'display-math': 'noteInsertDescMath',
  divider: 'noteInsertDescDivider',
}

function groupInsertItems(
  items: NoteEditorTool[],
): Array<{ group: NoteInsertGroup; items: NoteEditorTool[] }> {
  return INSERT_GROUP_ORDER.map((group) => ({
    group,
    items: items.filter((tool) => (INSERT_GROUP_OF[tool.key] ?? 'block') === group),
  })).filter((section) => section.items.length > 0)
}

/**
 * 行槛「+」：跟着空行的光标浮在左边，点开是块级插入菜单。
 * `/` 在空行上也打开同一份菜单，那时焦点在菜单里，方向键选、回车用。
 */
export function NoteGutter({
  caretLine,
  menu,
  onMenuChange,
  items,
  busy,
  widgets,
  onPickWidget,
}: NoteGutterProps) {
  const { t } = useI18n()
  const wrapRef = useRef<HTMLDivElement>(null)
  const plusRef = useRef<HTMLButtonElement>(null)
  const menuRef = useRef<HTMLElement>(null)
  const promptRef = useRef<HTMLInputElement>(null)
  const [prompting, setPrompting] = useState<NoteEditorTool | null>(null)
  const [promptValue, setPromptValue] = useState('')
  const [page, setPage] = useState<NoteInsertPage | null>(null)
  /** 从二级页退回来时把焦点还给入口那张卡，键盘能接着走。 */
  const returnFocusRef = useRef<string | null>(null)
  const [coords, setCoords] = useState<PopoverCoords | null>(null)
  const lastLineRef = useRef<SelectionAnchor | null>(null)
  const open = menu != null
  const { shown, leaving } = usePresence(open, MENU_LEAVE_MS)
  if (caretLine) lastLineRef.current = caretLine

  useEffect(() => {
    if (!shown) setCoords(null)
  }, [shown])

  useEffect(() => {
    if (!open) {
      setPrompting(null)
      setPromptValue('')
      setPage(null)
      return
    }
    const onDown = (event: MouseEvent) => {
      const target = event.target as Node
      if (wrapRef.current?.contains(target) || menuRef.current?.contains(target)) return
      onMenuChange(null)
    }
    document.addEventListener('mousedown', onDown)
    return () => document.removeEventListener('mousedown', onDown)
  }, [open, onMenuChange])

  // 菜单挂在 body 上、按视口定位：下面不够翻上去，左右夹住；滚动、缩放跟着走。
  const reposition = useCallback(() => {
    const trigger = plusRef.current
    const tip = menuRef.current
    if (!trigger || !tip) return
    const rect = trigger.getBoundingClientRect()
    if (tip.offsetWidth === 0 || tip.offsetHeight === 0) return
    const next = placePopover(
      rect,
      { width: tip.offsetWidth, height: tip.offsetHeight },
      { width: window.innerWidth, height: window.innerHeight },
    )
    setCoords((current) =>
      current &&
      current.top === next.top &&
      current.left === next.left &&
      current.placement === next.placement
        ? current
        : next,
    )
  }, [])

  useLayoutEffect(() => {
    if (!open || !shown) return
    reposition()
    const frame = requestAnimationFrame(reposition)
    return () => cancelAnimationFrame(frame)
  }, [open, shown, prompting, page, items.length, reposition])

  useEffect(() => {
    if (!open) return
    window.addEventListener('scroll', reposition, true)
    window.addEventListener('resize', reposition)
    return () => {
      window.removeEventListener('scroll', reposition, true)
      window.removeEventListener('resize', reposition)
    }
  }, [open, reposition])

  useEffect(() => {
    if (prompting) {
      promptRef.current?.focus()
      return
    }
    if (page) return
    const back = returnFocusRef.current
    if (back) {
      returnFocusRef.current = null
      menuRef.current
        ?.querySelector<HTMLButtonElement>(`.phantasi-note__menu-item[data-tool="${back}"]`)
        ?.focus()
      return
    }
    if (menu !== 'keys') return
    menuRef.current
      ?.querySelector<HTMLButtonElement>('.phantasi-note__menu-item')
      ?.focus()
  }, [menu, prompting, page])

  // 锚点没了（切到预览、编辑器失焦）菜单不能还开着：否则 hold 会把空状态钉住。
  useEffect(() => {
    if (open && !caretLine) onMenuChange(null)
  }, [open, caretLine, onMenuChange])

  const line = caretLine ?? lastLineRef.current
  if (!line || (!caretLine && !shown)) return null
  const placed = placeGutter(line, GUTTER_SIZE)

  /** 焦点离开整个「+」区域（不是在菜单项之间挪）就关掉。 */
  const closeIfFocusLeft = (event: FocusEvent<HTMLElement>) => {
    const next = event.relatedTarget as Node | null
    if (next && (wrapRef.current?.contains(next) || menuRef.current?.contains(next))) return
    // 用指针开的菜单焦点本来就在编辑器里，不按失焦算。
    if (menu === 'keys' || prompting || page) onMenuChange(null)
  }

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const search = menuRef.current?.querySelector<HTMLInputElement>('input[type="search"]')
    // 搜索框里左右 / Home / End 是挪光标，只有上下交给卡片导航。
    if (
      event.target === search &&
      !(event.key === 'ArrowDown' || event.key === 'ArrowUp')
    ) {
      return
    }
    const buttons = [
      ...(menuRef.current?.querySelectorAll<HTMLButtonElement>(
        '.phantasi-note__menu-item',
      ) ?? []),
    ]
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
    const go = (next: number) => {
      event.preventDefault()
      buttons[(next + buttons.length) % buttons.length]?.focus()
    }
    // 双列卡片：上下按几何找最近的一行里横向最近的那张，组间分隔线不占格。
    const vertical = (dir: 1 | -1) => {
      const current = buttons[index]?.getBoundingClientRect()
      if (!current) return go(0)
      const cx = current.left + current.width / 2
      let best = -1
      let bestScore = Infinity
      buttons.forEach((button, i) => {
        if (i === index) return
        const rect = button.getBoundingClientRect()
        const dy = dir > 0 ? rect.top - current.bottom : current.top - rect.bottom
        if (dy < -1) return
        const score = dy * 1000 + Math.abs(rect.left + rect.width / 2 - cx)
        if (score < bestScore) {
          bestScore = score
          best = i
        }
      })
      if (best < 0 && dir < 0 && search) {
        event.preventDefault()
        search.focus()
        return
      }
      go(best >= 0 ? best : dir > 0 ? 0 : buttons.length - 1)
    }
    if (event.key === 'ArrowDown') vertical(1)
    else if (event.key === 'ArrowUp') vertical(-1)
    else if (event.key === 'ArrowRight') go(index + 1)
    else if (event.key === 'ArrowLeft') go(index - 1)
    else if (event.key === 'Home') go(0)
    else if (event.key === 'End') go(buttons.length - 1)
  }

  const menuClass = [
    'phantasi-skin',
    'phantasi-note__menu',
    coords && !leaving ? 'is-ready' : '',
    leaving ? 'is-leaving' : '',
    coords ? `phantasi-note__menu--${coords.placement}` : '',
  ]
    .filter(Boolean)
    .join(' ')
  const menuStyle = coords ? { top: `${coords.top}px`, left: `${coords.left}px` } : undefined

  const popover = !shown
    ? null
    : createPortal(
        prompting ? (
          <form
            ref={menuRef as RefObject<HTMLFormElement>}
            className={`${menuClass} phantasi-note__menu--prompt`}
            style={menuStyle}
            onSubmit={(event) => {
              event.preventDefault()
              const value = promptValue.trim()
              if (!value) return
              onMenuChange(null)
              prompting.runWith?.(value)
            }}
            onBlur={closeIfFocusLeft}
          >
            <span className="phantasi-note__menu-icon">{prompting.icon}</span>
            <input
              ref={promptRef}
              className="phantasi-note__menu-input"
              type="url"
              inputMode="url"
              value={promptValue}
              placeholder={prompting.prompt}
              aria-label={prompting.label}
              onChange={(event) => setPromptValue(event.target.value)}
            />
          </form>
        ) : (
          <div
            ref={menuRef as RefObject<HTMLDivElement>}
            className={`${menuClass} phantasi-note__menu--cards${page ? ' phantasi-note__menu--page' : ''}`}
            style={menuStyle}
            role="menu"
            aria-label={t.phantasi.noteInsert}
            onKeyDown={onMenuKeyDown}
            onBlur={closeIfFocusLeft}
          >
            {page === 'widgets' ? (
              <NoteWidgetPage
                widgets={widgets}
                onBack={() => {
                  returnFocusRef.current = 'widget'
                  setPage(null)
                }}
                onPick={(type, size) => {
                  onMenuChange(null)
                  onPickWidget(type, size)
                }}
              />
            ) : (
              <>
              {groupInsertItems(items).map((section) => (
                <div key={section.group} className="phantasi-note__menu-group">
                  {section.items.map((tool) => {
                    const desc = INSERT_DESC[tool.key]
                    const shortcut = INSERT_SHORTCUT[tool.key]
                    return (
                      <button
                        key={tool.key}
                        type="button"
                        role="menuitem"
                        data-tool={tool.key}
                        className="phantasi-note__menu-item"
                        onMouseDown={(event) => event.preventDefault()}
                        onClick={() => {
                          if (tool.page) {
                            tool.run()
                            setPage(tool.page)
                            return
                          }
                          if (tool.prompt && tool.runWith) {
                            setPrompting(tool)
                            return
                          }
                          onMenuChange(null)
                          tool.run()
                        }}
                      >
                        <span className="phantasi-note__menu-icon">{tool.icon}</span>
                        <span className="phantasi-note__menu-text">
                          <span className="phantasi-note__menu-title">
                            <span className="phantasi-note__menu-label">{tool.label}</span>
                            {shortcut ? (
                              <kbd className="phantasi-note__menu-key">{shortcut}</kbd>
                            ) : null}
                          </span>
                          {desc ? (
                            <span className="phantasi-note__menu-desc">{t.phantasi[desc]}</span>
                          ) : null}
                        </span>
                      </button>
                    )
                  })}
                </div>
              ))}
              </>
            )}
          </div>
        ),
        document.body,
      )

  return (
    <div
      ref={wrapRef}
      className="phantasi-note__gutter"
      style={{ top: `${placed.top}px`, left: `${placed.left}px` }}
    >
      <button
        ref={plusRef}
        type="button"
        className={`phantasi-note__plus${open ? ' is-open' : ''}`}
        aria-label={t.phantasi.noteInsert}
        title={t.phantasi.noteInsert}
        aria-expanded={open}
        aria-haspopup="menu"
        disabled={busy}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => onMenuChange(open ? null : 'pointer')}
      >
        <Plus />
      </button>
      {popover}
    </div>
  )
}

/* ------------------------------------------------------------------ */

interface NoteFootBarProps {
  aiActions?: ReactNode
  chars: number
  pane: NoteEditorPane
  onPaneChange: (pane: NoteEditorPane) => void
}

/** 底栏：字数、AI 排版操作和编辑模式切换。 */
export function NoteFootBar({ chars, pane, onPaneChange, aiActions }: NoteFootBarProps) {
  const { t, format } = useI18n()
  return (
    <footer className="phantasi-note__foot">
      <div className="phantasi-note__foot-left">
        <span className="phantasi-note__count">
          {format(t.phantasi.noteCharCount, { count: chars })}
        </span>
        {aiActions ? <div className="phantasi-note__foot-actions">{aiActions}</div> : null}
      </div>
      <div className="phantasi-note__foot-right">
        <NoteSwitch<NoteEditorPane>
          className="phantasi-note__modes"
          ariaLabel={t.phantasi.noteTabWrite}
          value={pane}
          onChange={onPaneChange}
          options={[
            { value: 'write', label: t.phantasi.noteTabWrite },
            { value: 'visual', label: t.phantasi.noteTabVisual },
            { value: 'preview', label: t.phantasi.noteTabPreview },
          ]}
        />
      </div>
    </footer>
  )
}

/* ------------------------------------------------------------------ */

type NoteSettingsTab = 'publish' | 'details' | 'editor'

const SETTINGS_TABS: NoteSettingsTab[] = ['publish', 'details', 'editor']

interface NoteSettingsDrawerProps {
  /** 编辑器分页：默认视图（随账号）和编辑历史。 */
  editorSettings?: ReactNode
  companion?: ReactNode
  open: boolean
  onClose: () => void
  title: string
  docStatus: NoteEditorDocStatus
  lastError: string | null
  onPublish: () => void
  topic: string | null
  topicChoices: string[]
  onTopicChange: (topic: string | null) => void
  onCreateTopic: (name: string) => void
  publishedAt: number | null
  onPublishedAtChange: (ms: number | null) => void
  slugInput: string
  slugInvalid: boolean
  onSlugInput: (value: string) => void
  articleHref?: string | null
  scheduledAt: number | null
  onScheduledAtChange: (ms: number | null) => void
  canSchedule: boolean
  busy: boolean
  onSchedule: () => void
  onUnschedule: () => void
  cover: string | null
  coverPreview: string | null
  uploading: boolean
  onPickCover: () => void
  onClearCover: () => void
  canDelete: boolean
  onDelete: () => void
  authors: Array<{ user_id: number; label: string; owner: boolean }>
  addableAuthors: Array<{ value: string; label: string }>
  authorBusy: boolean
  onAddAuthor: (userId: number) => void
  onRemoveAuthor: (userId: number) => void
}

function useMinuteClock(active: boolean): number {
  const [now, setNow] = useState(Date.now)
  useEffect(() => {
    if (!active) return
    setNow(Date.now())
    const timer = window.setInterval(() => setNow(Date.now()), 30_000)
    return () => window.clearInterval(timer)
  }, [active])
  return now
}

function longWhen(ms: number, locale: string): string {
  return new Date(ms).toLocaleString(locale, {
    month: 'short',
    day: 'numeric',
    weekday: 'short',
    hour: '2-digit',
    minute: '2-digit',
  })
}

/**
 * 文章设置。顶上一行是访客在博客列表里看到的那一项（改什么先在这里看见），
 * 下面三页：发布（现在是什么状态、从这里能去哪）、信息（分类 / 封面 / 作者）、编辑器。
 *
 * 发布不是一条线：草稿可以直接发，也可以定时；定时的可以提前发、改时间、退回草稿；
 * 定时失败会退回草稿并留下原因；已发布的不能再定时，改动要再更新一次才到博客上。
 * 所以不画流程图，只摆当前状态和从这里能去的地方。
 */
export function NoteSettingsDrawer(props: NoteSettingsDrawerProps) {
  const {
    editorSettings,
    open,
    onClose,
    title,
    docStatus,
    topic,
    publishedAt,
    scheduledAt,
    coverPreview,
    uploading,
    onPickCover,
    authors,
  } = props
  const { t, locale } = useI18n()
  const [tab, setTab] = useState<NoteSettingsTab>('publish')
  /** 分页往哪边滑：跟着新旧两页在条上的先后。 */
  const [direction, setDirection] = useState<1 | -1>(1)
  const tabRefs = useRef<Partial<Record<NoteSettingsTab, HTMLButtonElement | null>>>({})
  const selectTab = (next: NoteSettingsTab, focus = false) => {
    if (next === tab) return
    setDirection(SETTINGS_TABS.indexOf(next) > SETTINGS_TABS.indexOf(tab) ? 1 : -1)
    setTab(next)
    if (focus) tabRefs.current[next]?.focus()
  }
  const onTabKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = SETTINGS_TABS.indexOf(tab)
    const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0
    if (step) {
      event.preventDefault()
      selectTab(SETTINGS_TABS[(index + step + SETTINGS_TABS.length) % SETTINGS_TABS.length]!, true)
    } else if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault()
      selectTab(SETTINGS_TABS[event.key === 'Home' ? 0 : SETTINGS_TABS.length - 1]!, true)
    }
  }

  const { shown, leaving } = useDrawerPresence(open)
  if (!shown) return null

  const statusLabel: Record<NoteEditorDocStatus, string> = {
    draft: t.phantasi.noteStatusDraft,
    scheduled: t.phantasi.noteStatusScheduled,
    published: t.phantasi.noteStatusPublished,
  }
  const when = docStatus === 'scheduled' ? scheduledAt : publishedAt
  const cardWhen =
    when != null && Number.isFinite(when) && when > 0
      ? new Date(when).toLocaleDateString(locale, { dateStyle: 'medium' })
      : null
  const tabLabel: Record<NoteSettingsTab, string> = {
    publish: t.phantasi.noteSettingsTabPublish,
    details: t.phantasi.noteSettingsTabDetails,
    editor: t.phantasi.noteSettingsTabEditor,
  }

  return (
    <aside
      className={`phantasi-note__drawer${leaving ? ' is-leaving' : ''}`}
      aria-label={t.phantasi.notePublishSettings}
      onKeyDown={(event) => {
        if (event.key !== 'Escape' || event.defaultPrevented) return
        event.stopPropagation()
        onClose()
      }}
    >
      <div className="phantasi-note__drawer-head">
        <h2 className="phantasi-note__drawer-title">{t.phantasi.notePublishSettings}</h2>
        <span key={docStatus} className={`phantasi-note__drawer-status is-${docStatus}`}>
          <span className="phantasi-note__drawer-status-dot" aria-hidden="true" />
          {statusLabel[docStatus]}
        </span>
        <NoteButton
          variant="quiet"
          icon={<X />}
          onClick={onClose}
          aria-label={t.phantasi.close}
          title={t.phantasi.close}
        />
      </div>

      {/* 博客列表里的那一项，缩成一行：封面、分类、标题、日期和作者。 */}
      <figure className="phantasi-note__postcard" aria-label={t.phantasi.noteSettingsPreview}>
        <button
          type="button"
          className="phantasi-note__postcard-cover"
          disabled={uploading}
          onClick={onPickCover}
          title={t.phantasi.noteCoverPick}
          aria-label={t.phantasi.noteCoverPick}
        >
          {coverPreview ? (
            <img key={coverPreview} src={coverPreview} alt="" className="phantasi-note__postcard-img" />
          ) : (
            <Image aria-hidden="true" />
          )}
        </button>
        <figcaption className="phantasi-note__postcard-body">
          <span key={topic ?? ''} className="phantasi-note__postcard-topic">
            {topic ? noteCategoryLabel(topic, t.phantasi) : t.phantasi.noteTopicNone}
          </span>
          <strong className="phantasi-note__postcard-title">
            {title.trim() || t.phantasi.noteHistoryUntitled}
          </strong>
          <span key={cardWhen ?? ''} className="phantasi-note__postcard-meta">
            {[cardWhen, authors.map((author) => author.label).join('、')].filter(Boolean).join(' · ')}
          </span>
        </figcaption>
      </figure>

      <div
        className="phantasi-note__drawer-tabs"
        role="tablist"
        aria-label={t.phantasi.notePublishSettings}
        onKeyDown={onTabKeyDown}
        style={
          {
            '--note-tab-count': SETTINGS_TABS.length,
            '--note-tab-index': SETTINGS_TABS.indexOf(tab),
          } as CSSProperties
        }
      >
        <span className="phantasi-note__drawer-tab-thumb" aria-hidden="true" />
        {SETTINGS_TABS.map((key) => (
          <button
            key={key}
            ref={(node) => { tabRefs.current[key] = node }}
            type="button"
            role="tab"
            id={`note-settings-tab-${key}`}
            aria-controls="note-settings-panel"
            aria-selected={tab === key}
            tabIndex={tab === key ? 0 : -1}
            className={`phantasi-note__drawer-tab${tab === key ? ' is-on' : ''}`}
            onClick={() => selectTab(key)}
          >
            {tabLabel[key]}
          </button>
        ))}
      </div>

      <div
        key={tab}
        id="note-settings-panel"
        role="tabpanel"
        aria-labelledby={`note-settings-tab-${tab}`}
        className="phantasi-note__drawer-panel"
        data-direction={direction > 0 ? 'forward' : 'back'}
      >
        {tab === 'publish' ? <NotePublishPanel {...props} /> : null}
        {tab === 'details' ? <NoteDetailsPanel {...props} /> : null}
        {tab === 'editor' ? editorSettings : null}
      </div>
    </aside>
  )
}

/* ---- 发布页 ---- */

function NotePublishPanel(props: NoteSettingsDrawerProps) {
  const { docStatus, canDelete, busy, onDelete } = props
  const { t } = useI18n()
  return (
    <>
      <div key={docStatus} className="phantasi-note__publish">
        {docStatus === 'draft' ? <NoteDraftPublish {...props} /> : null}
        {docStatus === 'scheduled' ? <NoteScheduledPublish {...props} /> : null}
        {docStatus === 'published' ? <NotePublishedPublish {...props} /> : null}
      </div>
      {/* 删除只对没发布的笔记有效，放在发布页最底下，不和主要动作抢位置。 */}
      {canDelete ? (
        <div className="phantasi-note__danger">
          <NoteButton variant="danger" icon={<Trash2 />} disabled={busy} onClick={onDelete}>
            {t.phantasi.noteDelete}
          </NoteButton>
        </div>
      ) : null}
    </>
  )
}

/** 草稿：只有一个问题——什么时候公开。二选一，用分段开关；下面跟着那条路自己的动作。 */
function NoteDraftPublish({
  lastError,
  onPublish,
  publishedAt,
  onPublishedAtChange,
  scheduledAt,
  onScheduledAtChange,
  canSchedule,
  busy,
  onSchedule,
}: NoteSettingsDrawerProps) {
  const { t } = useI18n()
  const now = useMinuteClock(true)
  const [route, setRoute] = useState<'now' | 'later'>('now')
  return (
    <>
      {lastError ? (
        <p className="phantasi-note__state-error" role="alert">
          <Clock aria-hidden="true" />
          <span>
            <strong>{t.phantasi.noteScheduleFailed}</strong>
            {lastError}
          </span>
        </p>
      ) : null}
      {canSchedule ? (
        <div className="phantasi-note__when">
        <span className="note-field__label">{t.phantasi.noteSettingsWhen}</span>
        <NoteSwitch
          className="phantasi-note__when-switch"
          ariaLabel={t.phantasi.noteSettingsWhen}
          value={route}
          options={[
            { value: 'now', label: t.phantasi.noteSettingsWhenNow },
            { value: 'later', label: t.phantasi.noteSettingsWhenLater },
          ]}
          onChange={setRoute}
        />
        <p className="note-field__hint">
          {route === 'now' ? t.phantasi.noteSettingsNowHint : t.phantasi.noteSettingsLaterHint}
        </p>
        </div>
      ) : null}
      <div key={route} className="phantasi-note__route-body">
        {route === 'now' ? (
          <>
            <NoteArticleDate value={publishedAt} onChange={onPublishedAtChange} />
            <NoteButton variant="solid" size="lg" icon={<Send />} disabled={busy} onClick={onPublish}>
              {t.phantasi.noteSettingsPublishNow}
            </NoteButton>
          </>
        ) : (
          <NoteSchedulePicker
            now={now}
            value={scheduledAt}
            onChange={onScheduledAtChange}
            busy={busy}
            onConfirm={onSchedule}
          />
        )}
      </div>
    </>
  )
}

/** 定时中：时间写大，倒计时跟着走；三条出路各一个按钮，名字就是去处。 */
function NoteScheduledPublish({
  onPublish,
  scheduledAt,
  onScheduledAtChange,
  busy,
  onSchedule,
  onUnschedule,
}: NoteSettingsDrawerProps) {
  const { t, locale } = useI18n()
  const now = useMinuteClock(true)
  const [rescheduling, setRescheduling] = useState(false)
  const due = scheduledAt != null && scheduledAt > 0 ? scheduledAt : null
  // 改时间时先记住原来的，取消就放回去，不把半路选的时间留在表单里。
  const original = useRef(scheduledAt)
  return (
    <section className="phantasi-note__state-card is-scheduled">
      <span className="phantasi-note__state-eyebrow">
        <Clock aria-hidden="true" />
        {t.phantasi.noteSettingsScheduledFor}
      </span>
      {due && !rescheduling ? (
        <>
          <strong className="phantasi-note__state-time">{longWhen(due, locale)}</strong>
          <span className="phantasi-note__state-sub">{noteRelativeUntil(due, now, locale)}</span>
        </>
      ) : null}
      {rescheduling ? (
        <NoteSchedulePicker
          now={now}
          value={scheduledAt}
          onChange={onScheduledAtChange}
          busy={busy}
          update
          onConfirm={() => {
            onSchedule()
            setRescheduling(false)
          }}
          onCancel={() => {
            onScheduledAtChange(original.current)
            setRescheduling(false)
          }}
        />
      ) : (
        <div className="phantasi-note__state-actions is-even">
          <NoteButton variant="solid" disabled={busy} onClick={onPublish}>
            {t.phantasi.noteSettingsPublishNow}
          </NoteButton>
          <NoteButton
            variant="outline"
            disabled={busy}
            onClick={() => {
              original.current = scheduledAt
              setRescheduling(true)
            }}
          >
            {t.phantasi.noteSettingsReschedule}
          </NoteButton>
          <NoteButton variant="quiet" disabled={busy} onClick={onUnschedule}>
            {t.phantasi.noteSettingsBackToDraft}
          </NoteButton>
        </div>
      )}
    </section>
  )
}

/**
 * 已发布：地址是最常用的东西，平时直接给复制和打开；改地址会让旧链接失效，收起来。
 * 正文、分类、封面、日期的修改只存进草稿，要再更新一次才到博客上。
 */
function NotePublishedPublish({
  onPublish,
  busy,
  publishedAt,
  onPublishedAtChange,
  slugInput,
  slugInvalid,
  onSlugInput,
  articleHref,
  companion,
}: NoteSettingsDrawerProps) {
  const { t } = useI18n()
  const [editingSlug, setEditingSlug] = useState(false)
  const [copied, setCopied] = useState(false)
  const [originalSlug] = useState(slugInput.trim())
  const slugChanged = slugInput.trim() !== originalSlug
  const path = `/blog/posts/${slugInput.trim() || originalSlug}/`
  useEffect(() => {
    if (!copied) return
    const timer = window.setTimeout(setCopied, 1600, false)
    return () => window.clearTimeout(timer)
  }, [copied])
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(new URL(articleHref || path, window.location.origin).href)
      setCopied(true)
    } catch {
      // 剪贴板不在我们控制内（权限、非安全上下文），失败就不提示「已复制」。
    }
  }
  return (
    <>
      <section className="phantasi-note__state-card is-published">
        <span className="phantasi-note__state-eyebrow">
          <Check aria-hidden="true" />
          {t.phantasi.noteSettingsLive}
        </span>
        {editingSlug ? (
          <NoteField
            label={t.studio.slug}
            hint={slugInvalid ? t.studio.slugInvalid : slugChanged ? t.phantasi.noteSettingsSlugWarn : undefined}
            htmlFor="note-slug"
          >
            <span className={`phantasi-note__slug${slugInvalid ? ' is-invalid' : ''}`}>
              <span className="phantasi-note__slug-prefix" aria-hidden="true">/blog/posts/</span>
              <input
                id="note-slug"
                autoFocus
                className="note-input"
                value={slugInput}
                spellCheck={false}
                autoCapitalize="off"
                autoCorrect="off"
                aria-invalid={slugInvalid || undefined}
                onChange={(event) => onSlugInput(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' && !slugInvalid) setEditingSlug(false)
                }}
              />
            </span>
          </NoteField>
        ) : (
          <div className="phantasi-note__address">
            <code className="phantasi-note__address-path" title={path}>{path}</code>
            <NoteButton
              variant="quiet"
              icon={copied ? <Check /> : <Copy />}
              onClick={() => { void copy() }}
              aria-label={t.phantasi.copy}
              title={copied ? t.phantasi.copied : t.phantasi.copy}
            />
            {articleHref && !slugInvalid ? (
              <a
                className="phantasi-note__address-open"
                href={articleHref}
                target="_blank"
                rel="noopener noreferrer"
                title={t.studio.viewArticle}
                aria-label={t.studio.viewArticle}
              >
                <ExternalLink aria-hidden="true" />
              </a>
            ) : null}
          </div>
        )}
        <div className="phantasi-note__state-actions">
          <NoteButton variant="solid" icon={<Send />} disabled={busy} onClick={onPublish}>
            {t.phantasi.noteSettingsUpdateBlog}
          </NoteButton>
          <NoteButton
            variant="quiet"
            aria-expanded={editingSlug}
            disabled={editingSlug && slugInvalid}
            onClick={() => setEditingSlug((open) => !open)}
          >
            {editingSlug ? t.phantasi.noteSettingsDone : t.phantasi.noteSettingsAddressEdit}
          </NoteButton>
        </div>
        <p className="note-field__hint">{t.phantasi.noteSettingsUpdateHint}</p>
      </section>
      <NoteArticleDate value={publishedAt} onChange={onPublishedAtChange} />
      {companion}
    </>
  )
}

/** 文章上显示的日期。平时一行字，点「修改」才展开时间框。 */
function NoteArticleDate({ value, onChange }: { value: number | null; onChange: (ms: number | null) => void }) {
  const { t, locale } = useI18n()
  const [editing, setEditing] = useState(false)
  const label =
    value != null && value > 0
      ? new Date(value).toLocaleString(locale, { dateStyle: 'medium', timeStyle: 'short' })
      : '—'
  return (
    <div className="phantasi-note__date-row">
      <div className="phantasi-note__date-line">
        <span className="note-field__label">{t.phantasi.noteSettingsArticleDate}</span>
        {editing ? null : <span className="phantasi-note__date-value">{label}</span>}
        <NoteButton
          variant="quiet"
          className="phantasi-note__date-toggle"
          aria-expanded={editing}
          onClick={() => setEditing((open) => !open)}
        >
          {editing ? t.phantasi.noteSettingsDone : t.phantasi.noteSettingsEdit}
        </NoteButton>
      </div>
      {editing ? (
        <div className="phantasi-note__date-edit">
          <NoteDateInput id="note-published-at" value={value} onChange={onChange} aria-label={t.phantasi.noteSettingsArticleDate} />
          <p className="note-field__hint">{t.phantasi.noteSettingsArticleDateHint}</p>
        </div>
      ) : null}
    </div>
  )
}

/**
 * 选定时时间：三个常用时间加「自定义」，一排四格；只有自定义才露出时间框。
 * 选中的时间直接写在确认按钮上，不另起一行摘要。
 */
function NoteSchedulePicker({
  now,
  value,
  onChange,
  busy,
  update = false,
  onConfirm,
  onCancel,
}: {
  now: number
  value: number | null
  onChange: (ms: number | null) => void
  busy: boolean
  update?: boolean
  onConfirm: () => void
  onCancel?: () => void
}) {
  const { t, locale, format } = useI18n()
  const presets = noteSchedulePresets(now)
  const presetLabel: Record<NoteSchedulePresetKey, string> = {
    hour: t.phantasi.noteSettingsPresetHour,
    tonight: t.phantasi.noteSettingsPresetTonight,
    tomorrowMorning: t.phantasi.noteSettingsPresetTomorrowMorning,
    tomorrowEvening: t.phantasi.noteSettingsPresetTomorrowEvening,
  }
  const matched = presets.find((preset) => preset.at === value)
  const [custom, setCustom] = useState(value != null && value > 0 && !matched)
  const future = value != null && value > now
  const status = future
    ? `${noteRelativeUntil(value, now, locale)} · ${t.phantasi.noteSettingsScheduleDateNote}`
    : value != null && value > 0
      ? t.phantasi.noteSchedulePast
      : t.phantasi.noteScheduleNeedTime
  return (
    <div className="phantasi-note__schedule">
      <div className="phantasi-note__presets" role="radiogroup" aria-label={t.phantasi.noteScheduledAt}>
        {presets.map((preset) => {
          const on = !custom && value === preset.at
          return (
            <button
              key={preset.key}
              type="button"
              role="radio"
              aria-checked={on}
              className={`phantasi-note__preset${on ? ' is-on' : ''}`}
              onClick={() => {
                setCustom(false)
                onChange(preset.at)
              }}
            >
              <span>{presetLabel[preset.key]}</span>
              <small>{new Date(preset.at).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })}</small>
            </button>
          )
        })}
        <button
          type="button"
          role="radio"
          aria-checked={custom}
          className={`phantasi-note__preset${custom ? ' is-on' : ''}`}
          onClick={() => setCustom(true)}
        >
          <span>{t.phantasi.noteSettingsPresetCustom}</span>
          <small aria-hidden="true">…</small>
        </button>
      </div>
      {custom ? (
        <div className="phantasi-note__schedule-custom">
          <NoteDateInput
            id="note-scheduled-at"
            value={value}
            onChange={onChange}
            aria-label={t.phantasi.noteScheduledAt}
          />
        </div>
      ) : null}
      <p className={`phantasi-note__schedule-summary${future ? ' is-ready' : ''}`} aria-live="polite">
        {status}
      </p>
      <div className="phantasi-note__schedule-actions">
        <NoteButton variant="solid" size="lg" icon={<Clock />} disabled={busy || !future} onClick={onConfirm}>
          {future
            ? format(update ? t.phantasi.noteSettingsScheduleUpdateAt : t.phantasi.noteSettingsScheduleAt, { when: longWhen(value, locale) })
            : update
              ? t.phantasi.noteSettingsScheduleUpdate
              : t.phantasi.noteSchedule}
        </NoteButton>
        {onCancel ? (
          <NoteButton variant="quiet" onClick={onCancel}>
            {t.phantasi.cancel}
          </NoteButton>
        ) : null}
      </div>
    </div>
  )
}

/* ---- 信息页 ---- */

function NoteDetailsPanel({
  topic,
  topicChoices,
  onTopicChange,
  onCreateTopic,
  cover,
  coverPreview,
  uploading,
  onPickCover,
  onClearCover,
  authors,
  addableAuthors,
  authorBusy,
  onAddAuthor,
  onRemoveAuthor,
}: NoteSettingsDrawerProps) {
  const { t } = useI18n()
  const [creating, setCreating] = useState(false)
  const [draftCategory, setDraftCategory] = useState('')
  const names = useMemo(() => {
    const list = [...topicChoices]
    if (topic && !list.includes(topic)) list.push(topic)
    return list
  }, [topic, topicChoices])
  const commitDraftCategory = () => {
    const name = normalizeNoteCategory(draftCategory)
    if (!name) return
    onCreateTopic(name)
    setDraftCategory('')
    setCreating(false)
  }
  const autoCover = cover == null

  return (
    <>
      {/* 分类是单选：一排芯片，点哪个是哪个；新建就地变成输入框。 */}
      <NoteSection title={t.phantasi.noteTopic}>
        <div className="phantasi-note__choice-chips" role="radiogroup" aria-label={t.phantasi.noteTopic}>
          {[null, ...names].map((name) => (
            <button
              key={name ?? ''}
              type="button"
              role="radio"
              aria-checked={topic === name}
              className={`phantasi-note__choice${topic === name ? ' is-on' : ''}${name == null ? ' is-none' : ''}`}
              onClick={() => onTopicChange(name)}
            >
              {topic === name ? <Check aria-hidden="true" /> : null}
              {name == null ? t.phantasi.noteTopicNone : noteCategoryLabel(name, t.phantasi)}
            </button>
          ))}
          {creating ? (
            <input
              autoFocus
              className="phantasi-note__choice-input"
              value={draftCategory}
              placeholder={t.phantasi.noteCategoryHint}
              aria-label={t.phantasi.noteCategoryNew}
              onChange={(event) => setDraftCategory(event.target.value)}
              onBlur={() => { if (!draftCategory.trim()) setCreating(false) }}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault()
                  commitDraftCategory()
                } else if (event.key === 'Escape') {
                  event.preventDefault()
                  event.stopPropagation()
                  setDraftCategory('')
                  setCreating(false)
                }
              }}
            />
          ) : (
            <button type="button" className="phantasi-note__choice is-add" onClick={() => setCreating(true)}>
              <Plus aria-hidden="true" />
              {t.phantasi.noteCategoryNew}
            </button>
          )}
        </div>
      </NoteSection>

      {/*
        封面是这一页唯一的图，给整宽。左上角写它从哪来（自动 / 指定），
        点图就换；指定的才有「改回自动」。正文没图时整块变成上传入口。
      */}
      <NoteSection title={t.phantasi.noteCover}>
        <button
          type="button"
          className={`phantasi-note__cover${coverPreview ? '' : ' is-empty'}`}
          disabled={uploading}
          aria-busy={uploading || undefined}
          onClick={onPickCover}
          aria-label={coverPreview ? t.phantasi.noteSettingsCoverReplace : t.phantasi.noteCoverPick}
        >
          {coverPreview ? (
            <>
              <img key={coverPreview} src={coverPreview} alt="" />
              <span className="phantasi-note__cover-badge">
                {autoCover ? `${t.phantasi.noteSettingsCoverAuto} · ${t.phantasi.noteCoverAuto}` : t.phantasi.noteSettingsCoverCustom}
              </span>
              <span className="phantasi-note__cover-hover" aria-hidden="true">
                <Replace />
                {t.phantasi.noteSettingsCoverReplace}
              </span>
            </>
          ) : (
            <span className="phantasi-note__cover-empty">
              <Image aria-hidden="true" />
              <strong>{t.phantasi.noteCoverPick}</strong>
              <small>{t.phantasi.noteSettingsCoverNone}</small>
            </span>
          )}
        </button>
        {!autoCover ? (
          <NoteButton variant="quiet" className="phantasi-note__cover-reset" disabled={uploading} onClick={onClearCover}>
            {t.phantasi.noteCoverClear}
          </NoteButton>
        ) : null}
      </NoteSection>

      {/* 作者：发起人在前、不能移除；加人是同一排里的一个芯片。 */}
      <NoteSection title={t.phantasi.noteAuthors}>
        <div className="note-chips">
          {authors.map((author) => (
            <NoteChip
              key={author.user_id}
              muted={author.owner}
              onDismiss={author.owner || authorBusy ? undefined : () => onRemoveAuthor(author.user_id)}
              dismissLabel={t.phantasi.noteAuthorRemove}
            >
              {author.owner ? `${author.label} · ${t.phantasi.noteAuthorOwner}` : author.label}
            </NoteChip>
          ))}
          {addableAuthors.length > 0 ? (
            <label className="phantasi-note__chip-add">
              <Plus aria-hidden="true" />
              <span>{t.phantasi.noteSettingsAdd}</span>
              <select
                value=""
                disabled={authorBusy}
                aria-label={t.phantasi.noteAuthorAdd}
                onChange={(event) => {
                  if (event.target.value) onAddAuthor(Number(event.target.value))
                }}
              >
                <option value="" disabled>
                  {t.phantasi.noteAuthorAdd}
                </option>
                {addableAuthors.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
          ) : null}
        </div>
        {authors.length <= 1 && addableAuthors.length === 0 ? (
          <p className="note-field__hint">{t.phantasi.noteAuthorEmpty}</p>
        ) : null}
      </NoteSection>
    </>
  )
}
