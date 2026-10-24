import type { CSSProperties, FocusEvent, KeyboardEvent, ReactNode, RefObject } from 'react'
import type { WidgetType } from '../../widgetGridTypes'
/**
 * 写作器的外壳：薄顶栏、标题下的一行元信息、选区浮动条、底栏、发布抽屉。
 * 控件全是 NoteControls 里自己的，不借设置页；这里不碰 phantasiApi。
 */

import type { NoteCollabPeer } from './noteCollab'
import type { NoteImageValues } from './NoteImageFields'
import type { PopoverCoords } from './notePopover'
import type { SelectionAnchor, VisualBlockKind } from './noteSelection'

import {
  LuImage as Image,
  LuPlus as Plus,
  LuReplace as Replace,
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
  NoteSelect,
  NoteSwitch,
} from './NoteControls'
import { NoteImageFields } from './NoteImageFields'
import { placePopover } from './notePopover'
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

interface NoteSettingsDrawerProps {
  editorSettings?: React.ReactNode
  companion?: React.ReactNode
  open: boolean
  onClose: () => void
  docStatus: NoteEditorDocStatus
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

/** 分类 / 时间 / 封面 / 定时 / 删除。写作时收起来。 */
export function NoteSettingsDrawer({
  editorSettings,
  companion,
  open,
  onClose,
  docStatus,
  topic,
  topicChoices,
  onTopicChange,
  onCreateTopic,
  publishedAt,
  onPublishedAtChange,
  slugInput,
  slugInvalid,
  onSlugInput,
  articleHref,
  scheduledAt,
  onScheduledAtChange,
  canSchedule,
  busy,
  onSchedule,
  onUnschedule,
  cover,
  coverPreview,
  uploading,
  onPickCover,
  onClearCover,
  canDelete,
  onDelete,
  authors,
  addableAuthors,
  authorBusy,
  onAddAuthor,
  onRemoveAuthor,
}: NoteSettingsDrawerProps) {
  const { t } = useI18n()
  const [draftCategory, setDraftCategory] = useState('')
  const topicOptions = useMemo(() => {
    const names = [...topicChoices]
    if (topic && !names.includes(topic)) names.push(topic)
    return [
      { value: '', label: t.phantasi.noteTopicNone },
      ...names.map((name) => ({
        value: name,
        label: noteCategoryLabel(name, t.phantasi),
      })),
    ]
  }, [t.phantasi, topic, topicChoices])
  const commitDraftCategory = () => {
    const name = normalizeNoteCategory(draftCategory)
    if (!name) return
    onCreateTopic(name)
    setDraftCategory('')
  }

  const { shown, leaving } = useDrawerPresence(open)
  if (!shown) return null
  return (
    <aside
      className={`phantasi-note__drawer${leaving ? ' is-leaving' : ''}`}
      aria-label={t.phantasi.notePublishSettings}
    >
      <div className="phantasi-note__drawer-head">
        <h2 className="phantasi-note__drawer-title">{t.phantasi.notePublishSettings}</h2>
        <NoteButton
          variant="quiet"
          icon={<X />}
          onClick={onClose}
          aria-label={t.phantasi.close}
          title={t.phantasi.close}
        />
      </div>

      {editorSettings}
      <NoteSection title={t.phantasi.noteAuthors} hint={authors.length === 0 ? t.phantasi.noteAuthorEmpty : undefined}>
        {authors.length > 0 ? (
          <div className="note-chips">
            {authors.map((author) => (
              <NoteChip
                key={author.user_id}
                muted={author.owner}
                onDismiss={
                  author.owner || authorBusy
                    ? undefined
                    : () => onRemoveAuthor(author.user_id)
                }
                dismissLabel={t.phantasi.noteAuthorRemove}
              >
                {author.owner
                  ? `${author.label} · ${t.phantasi.noteAuthorOwner}`
                  : author.label}
              </NoteChip>
            ))}
          </div>
        ) : null}
        {addableAuthors.length > 0 ? (
          <NoteSelect
            id="note-author-add"
            value=""
            options={[
              { value: '', label: t.phantasi.noteAuthorAdd },
              ...addableAuthors,
            ]}
            onChange={(value) => {
              if (!value) return
              onAddAuthor(Number(value))
            }}
          />
        ) : null}
      </NoteSection>

      <NoteSection title={t.phantasi.noteTopic}>
        <NoteSelect
          id="note-topic"
          value={topic ?? ''}
          options={topicOptions}
          onChange={(value) => onTopicChange(value || null)}
        />
        <div className="note-compose">
          <input
            className="note-input"
            value={draftCategory}
            placeholder={t.phantasi.noteCategoryHint}
            aria-label={t.phantasi.noteCategoryNew}
            onChange={(event) => setDraftCategory(event.target.value)}
            onKeyDown={(event) => {
              if (event.key !== 'Enter') return
              event.preventDefault()
              commitDraftCategory()
            }}
          />
          <NoteButton
            variant="outline"
            disabled={!normalizeNoteCategory(draftCategory)}
            onClick={commitDraftCategory}
          >
            {t.phantasi.noteCategoryNew}
          </NoteButton>
        </div>
      </NoteSection>

      {docStatus === 'published' ? (
        <NoteField
          label={t.studio.slug}
          hint={slugInvalid ? t.studio.slugInvalid : t.studio.slugHint}
          htmlFor="note-slug"
        >
          <input
            id="note-slug"
            className="note-input"
            value={slugInput}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            aria-invalid={slugInvalid || undefined}
            onChange={(event) => onSlugInput(event.target.value)}
          />
        </NoteField>
      ) : null}

      {docStatus === 'published' && articleHref ? (
        <a className="note-btn note-btn--outline" href={articleHref} target="_blank" rel="noopener noreferrer">
          <span className="note-btn__label">{t.studio.viewArticle}</span>
        </a>
      ) : null}

      {companion}

      <NoteField label={t.phantasi.notePublishedAt} htmlFor="note-published-at">
        <NoteDateInput
          id="note-published-at"
          value={publishedAt}
          onChange={onPublishedAtChange}
        />
      </NoteField>

      {docStatus === 'published' ? null : (
        <NoteField label={t.phantasi.noteScheduledAt} htmlFor="note-scheduled-at">
          <NoteDateInput
            id="note-scheduled-at"
            value={scheduledAt}
            onChange={onScheduledAtChange}
          />
          <div className="phantasi-note__drawer-actions">
            {docStatus === 'scheduled' ? (
              <NoteButton variant="outline" disabled={busy} onClick={onUnschedule}>
                {t.phantasi.noteUnschedule}
              </NoteButton>
            ) : (
              <NoteButton
                variant="outline"
                disabled={busy || !canSchedule}
                onClick={onSchedule}
              >
                {t.phantasi.noteSchedule}
              </NoteButton>
            )}
          </div>
        </NoteField>
      )}

      <NoteField
        label={t.phantasi.noteCover}
        hint={cover ? undefined : t.phantasi.noteCoverAuto}
      >
        <div className="phantasi-note__cover">
          <button
            type="button"
            className="phantasi-note__cover-card"
            disabled={uploading}
            onClick={onPickCover}
          >
            {coverPreview ? (
              <img src={coverPreview} alt="" className="phantasi-note__cover-thumb" />
            ) : (
              <span className="phantasi-note__cover-thumb phantasi-note__cover-thumb--empty">
                <Image />
              </span>
            )}
            <span className="phantasi-note__cover-action">
              {t.phantasi.noteCoverPick}
            </span>
          </button>
          {cover ? (
            <NoteButton variant="quiet" onClick={onClearCover}>
              {t.phantasi.noteCoverClear}
            </NoteButton>
          ) : null}
        </div>
      </NoteField>

      {canDelete ? (
        <div className="phantasi-note__drawer-foot">
          <NoteButton
            variant="danger"
            icon={<Trash2 />}
            disabled={busy}
            onClick={onDelete}
          >
            {t.phantasi.noteDelete}
          </NoteButton>
        </div>
      ) : null}
    </aside>
  )
}
