import type { ReactNode } from 'react'
import type { MediaAsset, MediaFilter } from '../../../services/mediaApi'
import type { ManagedListItem } from '../../settings/ManagedList'
import {
  LuFileText,
  LuImage,
  LuList,
  LuSquare,
  LuUpload,
} from '@lib/icons'
import { useMemo, useRef, useState } from 'react'
import { useConfigI18n as useI18n } from '../../../contexts/I18nContext'
import { draftMediaSrc } from '../../../services/mediaApi'
import { InputItem, ManagedList, SettingsButton } from '../../settings'
import {
  formatWorkbenchBytes,
  WORKBENCH_MEDIA_FORMATS,
  workbenchMediaFormatKey,
  workbenchMediaFormatLabel,
  workbenchMediaRefLabel,
} from '../logic/workbench'
import { noteScheduleLabel } from '../notes/noteBoard'
import { PhantasiWorkbenchIcon } from '../ui/PhantasiWorkbenchIcon'
import { AuthenticatedMedia } from './AuthenticatedMedia'
import { MediaEditorDialog } from './MediaEditorDialog'
import { PageAction, WorkbenchPage } from './PhantasiWorkbenchChrome'
import './WorkbenchMediaCards.css'

export function WorkbenchMediaPane({
  active,
  back,
  media,
  mediaLoading,
  mediaFilter,
  onMediaFilter,
  mediaHasMore,
  onLoadMoreMedia,
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
  mediaFilter: MediaFilter
  onMediaFilter: (filter: MediaFilter) => void
  mediaHasMore: boolean
  onLoadMoreMedia: () => void
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
  const { kind: mediaKind, format: mediaFormat, query: mediaQuery } = mediaFilter
  const [selectedId, setSelectedId] = useState<number | null>(null)
  const [mediaLayout, setMediaLayout] = useState<'list' | 'grid'>('grid')
  const mediaFormatOptions = useMemo(
    () => [
      { key: 'all', label: phantasi.workbenchMediaAll },
      ...WORKBENCH_MEDIA_FORMATS.map((key) => ({
        key,
        label: workbenchMediaFormatLabel(key, phantasi.workbenchMediaFormatOther),
      })),
    ],
    [phantasi.workbenchMediaAll, phantasi.workbenchMediaFormatOther],
  )

  const mediaItems = useMemo<ManagedListItem[]>(
    () =>
      media.map((item) => {
        const src = draftMediaSrc(item) || undefined
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
            {
              label: item.exposure === 'public'
                ? phantasi.workbenchMediaPublic
                : phantasi.workbenchMediaPrivate,
              tone: item.exposure === 'public' ? 'active' : 'muted',
            },
          ],
          className: 'phantasi-workbench__hover-actions phantasi-workbench__media',
          leading: (
            <AuthenticatedMedia src={src} video={video} className="phantasi-workbench__thumb" />
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
          onChange={query => onMediaFilter({ ...mediaFilter, query })}
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
            value: mediaFormat,
            onChange: (key) =>
              onMediaFilter({ ...mediaFilter, format: key }),
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
              onMediaFilter({ ...mediaFilter, kind: key as MediaFilter['kind'] }),
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
          mediaKind !== 'all' || mediaFormat !== 'all' || mediaQuery.trim()
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
