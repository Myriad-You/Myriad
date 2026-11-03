import type { DragEvent, ReactNode } from 'react'
import type { MediaAsset, MediaFilter } from '../../../services/mediaApi'
import type { ManagedListItem } from '../../settings/ManagedList'
import {
  LuFileText,
  LuImage,
  LuList,
  LuSquare,
  LuUpload,
} from '@lib/icons'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useConfigI18n as useI18n } from '../../../contexts/I18nContext'
import { mediaAssetOrigin, mediaAssetSrc } from '../../../services/mediaApi'
import { MediaPreview } from '../../media/MediaPreview'
import { ManagedList, SettingsButton } from '../../settings'
import { isOutsideFileDrag, WORKBENCH_MEDIA_ACCEPT } from '../logic/mediaUpload'
import {
  formatWorkbenchBytes,
  WORKBENCH_MEDIA_FORMATS,
  workbenchMediaFormatKey,
  workbenchMediaFormatLabel,
  workbenchMediaRefLabel,
} from '../logic/workbench'
import { noteScheduleLabel } from '../notes/noteBoard'
import { MediaEditorDialog } from './MediaEditorDialog'
import { PageAction, WorkbenchPage, WorkbenchSearch } from './PhantasiWorkbenchChrome'
import './WorkbenchMediaCards.css'

export function WorkbenchMediaPane({
  active,
  title,
  back,
  filters,
  media,
  mediaFormats,
  mediaLoading,
  mediaFilter,
  onMediaFilter,
  mediaHasMore,
  onLoadMoreMedia,
  busy,
  uploadProgress,
  onUpload,
  onDeleteMedia,
  onMediaSaved,
}: {
  active: boolean
  title?: string
  back: ReactNode
  /** 窗口工具条里额外的筛选，放在搜索下面一行。 */
  filters?: ReactNode
  media: MediaAsset[]
  mediaFormats?: readonly string[]
  mediaLoading: boolean
  mediaFilter: MediaFilter
  onMediaFilter: (filter: MediaFilter) => void
  mediaHasMore: boolean
  onLoadMoreMedia: () => void
  busy: boolean
  /** 正在传第几个、一共几个；没在传是 null。 */
  uploadProgress?: { current: number; total: number } | null
  /** 一次给一批，按顺序传由调用方负责。 */
  onUpload: (files: File[]) => void
  onDeleteMedia: (id: number) => void
  onMediaSaved?: (item: MediaAsset) => void
}) {
  const { t, locale, format } = useI18n()
  const phantasi = t.phantasi
  const fileRef = useRef<HTMLInputElement>(null)
  const [dropping, setDropping] = useState(false)
  // 进出子元素都会触发 dragenter/dragleave，数层数才知道是不是真离开了。
  const dragDepth = useRef(0)
  // 页面里自己拖起来的（文字、缩略图）不算上传。
  const dragFromPage = useRef(false)
  useEffect(() => {
    const start = () => {
      dragFromPage.current = true
    }
    const end = () => {
      dragFromPage.current = false
    }
    document.addEventListener('dragstart', start)
    document.addEventListener('dragend', end)
    return () => {
      document.removeEventListener('dragstart', start)
      document.removeEventListener('dragend', end)
    }
  }, [])
  const { origin: mediaOrigin, format: mediaFormat, query: mediaQuery } = mediaFilter
  const [selectedId, setSelectedId] = useState<number | null>(null)
  const [mediaLayout, setMediaLayout] = useState<'list' | 'grid'>('grid')
  const visibleFormats = useMemo(() => {
    const present = new Set(mediaFormats ?? media.map(workbenchMediaFormatKey))
    return WORKBENCH_MEDIA_FORMATS.filter(
      (key) => present.has(key) || key === mediaFormat,
    )
  }, [media, mediaFormat, mediaFormats])
  const mediaFormatOptions = useMemo(
    () => [
      { key: 'all', label: phantasi.workbenchMediaAll },
      ...visibleFormats.map((key) => ({
        key,
        label: workbenchMediaFormatLabel(key, phantasi.workbenchMediaFormatOther),
      })),
    ],
    [phantasi.workbenchMediaAll, phantasi.workbenchMediaFormatOther, visibleFormats],
  )

  const mediaItems = useMemo<ManagedListItem[]>(
    () =>
      media.map((item) => {
        const src = mediaAssetSrc(item) || undefined
        const inUse = item.references.length > 0
        const refs = workbenchMediaRefLabel(item.references, {
          notes: phantasi.workbenchMediaRefNotes,
          articles: phantasi.workbenchMediaRefArticles,
          sticker: phantasi.workbenchMediaRefSticker,
          channel: phantasi.workbenchMediaRefChannel,
          persona: phantasi.workbenchMediaRefPersona,
          site: phantasi.workbenchMediaRefSite,
        })
        const formatKey = workbenchMediaFormatKey(item)
        const format = workbenchMediaFormatLabel(
          formatKey,
          phantasi.workbenchMediaFormatOther,
        )
        const size = formatWorkbenchBytes(item.size)
        const when = noteScheduleLabel(item.created_at, locale)
        const origin = mediaAssetOrigin(item)
        const originLabel =
          origin === 'generated'
            ? phantasi.workbenchMediaGeneratedKind
            : origin === 'kept'
              ? phantasi.workbenchMediaKeptKind
              : origin === 'channel'
                ? phantasi.workbenchMediaChannelKind
                : origin === 'import'
                  ? phantasi.workbenchMediaImportKind
                  : phantasi.workbenchMediaUploadKind
        const video = item.mime.startsWith('video/')
        return {
          id: item.id,
          title: item.name,
          subtitle: [size, when].filter(Boolean).join(' · '),
          meta: inUse
            ? refs || phantasi.workbenchMediaInUse
            : phantasi.workbenchMediaUnused,
          badges: [
            { label: format, tone: video ? 'warn' : 'muted' },
            {
              label: originLabel,
              tone: origin === 'generated' || origin === 'kept' ? 'active' : 'muted',
            },
            {
              label: item.exposure === 'public'
                ? phantasi.workbenchMediaPublic
                : phantasi.workbenchMediaPrivate,
              tone: item.exposure === 'public' ? 'active' : 'muted',
            },
          ],
          className: 'phantasi-workbench__hover-actions phantasi-workbench__media',
          leading: (
            <MediaPreview src={src} video={video} className="phantasi-workbench__thumb" />
          ),
          renderHit: ({ leading, main }) => <button type="button" className="managed-list-row-hit" aria-label={item.name} onClick={() => setSelectedId(item.id)}>{leading}{main}</button>,
          actions: [
            {
              key: 'delete',
              label: phantasi.delete,
              variant: 'danger' as const,
              confirm: phantasi.workbenchDeleteMediaConfirm,
              onClick: () => onDeleteMedia(item.id),
              disabled: busy || inUse,
              title: inUse ? refs || phantasi.workbenchMediaInUse : undefined,
            },
          ],
        }
      }),
    [phantasi, busy, locale, onDeleteMedia, media],
  )

  const selectedIndex = media.findIndex(item => item.id === selectedId)
  const selected = media[selectedIndex]
  if (!active) return null

  const fileDrag = (event: DragEvent) =>
    isOutsideFileDrag(Array.from(event.dataTransfer.types), dragFromPage.current)

  return (
    <WorkbenchPage
      title={title ?? phantasi.workbenchMedia}
      back={back}
      filters={filters}
      drop={{
        onDragEnter: (event) => {
          if (!fileDrag(event)) return
          event.preventDefault()
          dragDepth.current += 1
          setDropping(true)
        },
        onDragOver: (event) => {
          if (!fileDrag(event)) return
          // 不拦下来浏览器会直接打开这个文件；忙的时候拦下但不收。
          event.preventDefault()
          event.dataTransfer.dropEffect = busy ? 'none' : 'copy'
        },
        onDragLeave: (event) => {
          if (!fileDrag(event)) return
          dragDepth.current = Math.max(0, dragDepth.current - 1)
          if (dragDepth.current === 0) setDropping(false)
        },
        onDrop: (event) => {
          if (!fileDrag(event)) return
          event.preventDefault()
          dragDepth.current = 0
          setDropping(false)
          const files = Array.from(event.dataTransfer.files)
          if (!busy && files.length > 0) onUpload(files)
        },
        overlay: dropping && !busy ? (
          <div className="workbench-media-drop" aria-hidden>
            <span className="workbench-media-drop__label">
              <LuUpload />
              {phantasi.workbenchMediaDrop}
            </span>
          </div>
        ) : null,
      }}
      search={
        <WorkbenchSearch
          value={mediaQuery}
          onChange={query => onMediaFilter({ ...mediaFilter, query })}
          placeholder={phantasi.workbenchSearchMedia}
        />
      }
      action={
        <PageAction
          label={uploadProgress
            ? format(phantasi.workbenchUploading, uploadProgress)
            : phantasi.workbenchUpload}
          description={phantasi.workbenchUploadHint}
          icon={<LuImage />}
          primary
          disabled={busy}
          loading={uploadProgress != null}
          onPick={() => fileRef.current?.click()}
        />
      }
    >
      <ManagedList
        layout={mediaLayout}
        filterGroups={[
          ...(visibleFormats.length > 0
            ? [
                {
                  label: phantasi.workbenchMediaFormat,
                  icon: <LuFileText />,
                  ariaLabel: phantasi.workbenchMediaFormat,
                  options: mediaFormatOptions,
                  value: mediaFormat,
                  onChange: (key: string) =>
                    onMediaFilter({ ...mediaFilter, format: key }),
                },
              ]
            : []),
          {
            label: phantasi.workbenchMediaLayout,
            icon: <LuSquare />,
            ariaLabel: phantasi.workbenchMediaLayout,
            options: [
              {
                key: 'list',
                label: phantasi.workbenchMediaLayoutList,
                icon: <LuList />,
              },
              {
                key: 'grid',
                label: phantasi.workbenchMediaLayoutGrid,
                icon: <LuSquare />,
              },
            ],
            value: mediaLayout,
            onChange: (key) =>
              setMediaLayout(key === 'list' ? 'list' : 'grid'),
          },
        ]}
        queryCollapsible={false}
        queryChrome="plain"
        loading={mediaLoading && media.length === 0}
        working={busy}
        items={mediaItems}
        emptyText={
          mediaOrigin !== 'all' || mediaFormat !== 'all' || mediaQuery.trim()
            ? phantasi.workbenchMediaKindEmpty
            : phantasi.workbenchMediaEmpty
        }
        maxHeight={null}
        footer={mediaHasMore ? (
          <SettingsButton variant="ghost" size="sm" loading={mediaLoading} onClick={onLoadMoreMedia}>
            {t.config.managedListShowMore}
          </SettingsButton>
        ) : undefined}
      />
      {selected && <MediaEditorDialog
        key={selected.id}
        item={selected}
        onClose={() => setSelectedId(null)}
        onPrevious={selectedIndex > 0 ? () => setSelectedId(media[selectedIndex - 1].id) : undefined}
        onNext={selectedIndex + 1 < media.length ? () => setSelectedId(media[selectedIndex + 1].id) : undefined}
        onSaved={item => onMediaSaved?.(item)}
                   />}
      <input
        ref={fileRef}
        type="file"
        accept={WORKBENCH_MEDIA_ACCEPT}
        multiple
        hidden
        onChange={(event) => {
          const files = Array.from(event.target.files ?? [])
          event.target.value = ''
          if (files.length > 0) onUpload(files)
        }}
      />
    </WorkbenchPage>
  )
}
