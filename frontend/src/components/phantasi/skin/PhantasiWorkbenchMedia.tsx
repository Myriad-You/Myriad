import type { ReactNode } from 'react'
import type { MediaAsset } from '../../../services/mediaApi'
import type { ManagedListItem } from '../../settings/ManagedList'
import type {
  WorkbenchMediaFormatFilter,
  WorkbenchMediaKindFilter,
} from '../logic/workbench'
import {
  LuFileText,
  LuImage,
  LuList,
  LuSquare,
  LuUpload,
} from '@lib/icons'
import { useMemo, useRef, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { InputItem, ManagedList } from '../../settings'
import { mediaPointerUrl } from '../logic/mediaPointer'
import {
  collectWorkbenchMediaFormats,
  filterWorkbenchMedia,
  formatWorkbenchBytes,
  workbenchMediaFormatKey,
  workbenchMediaFormatLabel,
  workbenchMediaRefLabel,
} from '../logic/workbench'
import { noteScheduleLabel } from '../notes/noteBoard'
import { PhantasiWorkbenchIcon } from '../ui/PhantasiWorkbenchIcon'
import { MediaEditorDialog } from './MediaEditorDialog'
import { PageAction, WorkbenchPage } from './PhantasiWorkbenchChrome'
import { Thumb } from './PhantasiWorkbenchHome'
import './WorkbenchMediaCards.css'

export function WorkbenchMediaPane({
  active,
  back,
  media,
  mediaLoading,
  busy,
  guide,
  guidePath,
  onUpload,
  onDeleteMedia,
  onMediaSaved,
}: {
  active: boolean
  back: ReactNode
  media: MediaAsset[]
  mediaLoading: boolean
  busy: boolean
  guide?: ReactNode
  guidePath?: string
  onUpload: (file: File) => void
  onDeleteMedia: (id: number) => void
  onMediaSaved?: (item: MediaAsset) => void
}) {
  const { t, locale } = useI18n()
  const phantasi = t.phantasi
  const fileRef = useRef<HTMLInputElement>(null)
  const [mediaKind, setMediaKind] = useState<WorkbenchMediaKindFilter>('all')
  const [mediaFormat, setMediaFormat] =
    useState<WorkbenchMediaFormatFilter>('all')
  const [selectedId, setSelectedId] = useState<number | null>(null)
  const [mediaQuery, setMediaQuery] = useState('')
  const [mediaLayout, setMediaLayout] = useState<'list' | 'grid'>('grid')
  const mediaFormats = useMemo(
    () => collectWorkbenchMediaFormats(media),
    [media],
  )
  const mediaFormatOptions = useMemo(
    () => [
      { key: 'all', label: phantasi.workbenchMediaAll },
      ...mediaFormats.map((key) => ({
        key,
        label: workbenchMediaFormatLabel(key, phantasi.workbenchMediaFormatOther),
      })),
    ],
    [phantasi.workbenchMediaAll, phantasi.workbenchMediaFormatOther, mediaFormats],
  )
  const resolvedMediaFormat = mediaFormatOptions.some(
    (opt) => opt.key === mediaFormat,
  )
    ? mediaFormat
    : 'all'
  const visibleMedia = useMemo(
    () =>
      filterWorkbenchMedia(media, {
        kind: mediaKind,
        format: resolvedMediaFormat,
        query: mediaQuery,
      }),
    [media, mediaKind, mediaQuery, resolvedMediaFormat],
  )

  const mediaItems = useMemo<ManagedListItem[]>(
    () =>
      visibleMedia.map((item) => {
        const src = mediaPointerUrl(item.url) ?? undefined
        const inUse = item.references.length > 0
        const refs = workbenchMediaRefLabel(item.references, {
          notes: phantasi.workbenchNotes,
          articles: phantasi.articles,
          site: phantasi.workbenchMediaRefSite,
        })
        const formatKey = workbenchMediaFormatKey(item)
        const format = workbenchMediaFormatLabel(
          formatKey,
          phantasi.workbenchMediaFormatOther,
        )
        const size = formatWorkbenchBytes(item.size)
        const when = noteScheduleLabel(item.created_at, locale)
        const kind =
          item.kind === 'generated'
            ? phantasi.workbenchMediaGeneratedKind
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
              label: kind,
              tone: item.kind === 'generated' ? 'active' : 'muted',
            },
          ],
          className: 'phantasi-workbench__hover-actions phantasi-workbench__media',
          leading: <Thumb src={src} video={video} />,
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
    [phantasi, busy, locale, onDeleteMedia, visibleMedia],
  )

  const selectedIndex = visibleMedia.findIndex(item => item.id === selectedId)
  const selected = visibleMedia[selectedIndex]
  if (!active) return null

  return (
    <WorkbenchPage
      title={phantasi.workbenchMedia}
      icon={<PhantasiWorkbenchIcon kind="media" />}
      back={back}
      guide={guide}
      guidePath={guidePath}
      search={
        <InputItem
          itemKey="workbench-media-search"
          label={phantasi.workbenchSearchMedia}
          value={mediaQuery}
          onChange={setMediaQuery}
          placeholder={phantasi.workbenchSearchMedia}
          inputType="search"
          size="sm"
          layout="vertical"
          autoComplete="off"
          className="phantasi-workbench__title-search"
        />
      }
      action={
        <PageAction
          label={phantasi.workbenchUpload}
          description={phantasi.workbenchUploadHint}
          icon={<LuImage />}
          disabled={busy}
          onPick={() => fileRef.current?.click()}
        />
      }
    >
      <ManagedList
        layout={mediaLayout}
        filterGroups={[
          {
            label: phantasi.workbenchMediaFormat,
            icon: <LuFileText />,
            ariaLabel: phantasi.workbenchMediaFormat,
            options: mediaFormatOptions,
            value: resolvedMediaFormat,
            onChange: (key) =>
              setMediaFormat(key as WorkbenchMediaFormatFilter),
          },
          {
            label: phantasi.workbenchMediaSource,
            icon: <LuUpload />,
            ariaLabel: phantasi.workbenchMediaSource,
            options: [
              { key: 'all', label: phantasi.workbenchMediaAll },
              { key: 'upload', label: phantasi.workbenchMediaUploadKind },
              {
                key: 'generated',
                label: phantasi.workbenchMediaGeneratedKind,
              },
            ],
            value: mediaKind,
            onChange: (key) =>
              setMediaKind(key as WorkbenchMediaKindFilter),
          },
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
          media.length === 0
            ? phantasi.workbenchMediaEmpty
            : phantasi.workbenchMediaKindEmpty
        }
        maxHeight={null}
      />
      {selected && <MediaEditorDialog
        key={selected.id}
        item={selected}
        onClose={() => setSelectedId(null)}
        onPrevious={selectedIndex > 0 ? () => setSelectedId(visibleMedia[selectedIndex - 1].id) : undefined}
        onNext={selectedIndex + 1 < visibleMedia.length ? () => setSelectedId(visibleMedia[selectedIndex + 1].id) : undefined}
        onSaved={item => onMediaSaved?.(item)}
                   />}
      <input
        ref={fileRef}
        type="file"
        accept="image/jpeg,image/png,image/gif,image/webp,video/mp4,video/webm,video/quicktime"
        hidden
        onChange={(event) => {
          const file = event.target.files?.[0]
          event.target.value = ''
          if (file) onUpload(file)
        }}
      />
    </WorkbenchPage>
  )
}
