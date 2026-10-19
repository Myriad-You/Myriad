import type { ReactNode } from 'react'
import type { PhantasiNoteDoc } from '../../../types/phantasi'
import type {
  ManagedListItem,
  ManagedListTone,
} from '../../settings/ManagedList'
import type { WorkbenchNoteStatusFilter, WorkbenchNoteStatusKey } from '../logic/workbench'
import { LuClipboardList, LuNotebookPen, LuTag, LuUser } from '@lib/icons'
import { useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { ManagedList } from '../../settings'
import { BatchCategoryPick } from '../BatchCategoryPick'
import { getImageUrl } from '../constants'
import {
  collectWorkbenchNoteAuthors,
  collectWorkbenchNoteTopics,
  filterWorkbenchNotes,
  workbenchNoteAuthorName,
  workbenchNoteCover,
  workbenchNoteListExcerpt,
  workbenchNoteOpen,
  workbenchNoteStatusFilter,
  workbenchNoteStatusKey,
  workbenchNoteWhen,
} from '../logic/workbench'
import { noteScheduleLabel } from '../notes/noteBoard'
import { listSelectChrome, useListSelection } from '../useListSelection'
import { PageAction, WorkbenchPage, WorkbenchSearch } from './PhantasiWorkbenchChrome'
import { WorkbenchThumb } from './WorkbenchThumb'

function noteTone(
  key: WorkbenchNoteStatusKey,
  failed = false,
): ManagedListTone {
  if (failed) return 'danger'
  if (key === 'noteStatusScheduled') return 'warn'
  if (key === 'noteStatusPublished') return 'success'
  return 'muted'
}

export function WorkbenchNotesPane({
  active,
  back,
  docs,
  notesLoading,
  busy,
  noteCategories,
  onWrite,
  onOpenNote,
  onDeleteNotes,
  onUnschedule,
  onAssignNotes,
  onOpenCategories,
}: {
  active: boolean
  back: ReactNode
  docs: PhantasiNoteDoc[]
  notesLoading: boolean
  busy: boolean
  noteCategories: readonly string[]
  onWrite: () => void
  onOpenNote: (open: ReturnType<typeof workbenchNoteOpen>) => void
  onDeleteNotes: (docs: PhantasiNoteDoc[]) => void | Promise<boolean>
  onUnschedule: (id: number, revision: number) => void
  onAssignNotes?: (docs: PhantasiNoteDoc[], category: string) => void
  onOpenCategories: () => void
}) {
  const { t, locale, format } = useI18n()
  const phantasi = t.phantasi
  const [noteQuery, setNoteQuery] = useState('')
  const [noteStatus, setNoteStatus] = useState<WorkbenchNoteStatusFilter>('all')
  const [noteTopic, setNoteTopic] = useState('all')
  const [noteAuthor, setNoteAuthor] = useState('all')
  const noteStatuses = useMemo(() => {
    const present = new Set(docs.map(workbenchNoteStatusFilter))
    return (['draft', 'scheduled', 'published'] as const).filter(
      (key) => present.has(key) || key === noteStatus,
    )
  }, [docs, noteStatus])
  const noteTopics = useMemo(() => collectWorkbenchNoteTopics(docs), [docs])
  const noteTopicOptions = useMemo(
    () => [
      { key: 'all', label: phantasi.noteCategoryAll },
      ...noteTopics.map((name) => ({ key: name, label: name })),
    ],
    [phantasi.noteCategoryAll, noteTopics],
  )
  const resolvedNoteTopic = noteTopicOptions.some((opt) => opt.key === noteTopic)
    ? noteTopic
    : 'all'
  const noteAuthors = useMemo(
    () => collectWorkbenchNoteAuthors(docs, phantasi.anonymousUser),
    [docs, phantasi.anonymousUser],
  )
  const noteAuthorOptions = useMemo(
    () => [{ key: 'all', label: phantasi.noteCategoryAll }, ...noteAuthors],
    [noteAuthors, phantasi.noteCategoryAll],
  )
  const resolvedNoteAuthor = noteAuthorOptions.some(
    (opt) => opt.key === noteAuthor,
  )
    ? noteAuthor
    : 'all'
  const visibleNotes = useMemo(
    () =>
      filterWorkbenchNotes(docs, {
        status: noteStatus,
        query: noteQuery,
        topic: resolvedNoteTopic === 'all' ? null : resolvedNoteTopic,
        author: resolvedNoteAuthor === 'all' ? null : resolvedNoteAuthor,
      }),
    [docs, noteQuery, noteStatus, resolvedNoteAuthor, resolvedNoteTopic],
  )
  const noteIds = useMemo(
    () => visibleNotes.map((doc) => doc.id),
    [visibleNotes],
  )
  const notesSelect = useListSelection(noteIds)

  useEffect(() => {
    if (!active) notesSelect.exit()
  }, [active, notesSelect.exit])

  const notesSelectBar = listSelectChrome({
    selecting: notesSelect.selecting,
    picked: notesSelect.picked,
    total: notesSelect.total,
    allOn: notesSelect.allOn,
    busy,
    labels: {
      edit: phantasi.edit,
      selectAll: phantasi.selectAll,
      deselectAll: phantasi.deselectAll,
      deleteSelected: phantasi.deleteSelected,
      deleteConfirm: format(phantasi.workbenchDeleteSelectedNotesConfirm, {
        count: notesSelect.picked,
      }),
      exitEdit: phantasi.exitEdit,
      selectedLabel: phantasi.editMode,
    },
    onEnter: notesSelect.enter,
    onExit: notesSelect.exit,
    onSelectAll: notesSelect.selectAll,
    onDelete: () => {
      const picked = visibleNotes.filter((doc) =>
        notesSelect.selected.has(doc.id),
      )
      if (picked.length === 0) return
      void Promise.resolve(onDeleteNotes(picked)).then((ok) => {
        if (ok !== false) notesSelect.exit()
      })
    },
  })

  const noteItems = useMemo<ManagedListItem[]>(
    () =>
      visibleNotes.map((doc) => {
        const statusKey = workbenchNoteStatusKey(doc)
        const when = noteScheduleLabel(workbenchNoteWhen(doc), locale)
        const excerpt = workbenchNoteListExcerpt(doc)
        const author = workbenchNoteAuthorName(doc, '')
        const facts = [author || null, doc.topic, when].filter(Boolean).join(' · ')
        const picking = notesSelect.selecting
        return {
          id: doc.id,
          title: doc.title.trim() || phantasi.workbenchNoteUntitled,
          subtitle: excerpt || facts || undefined,
          meta: excerpt && facts ? facts : undefined,
          badge: {
            label: phantasi[statusKey],
            tone: noteTone(statusKey, Boolean(doc.last_error)),
          },
          className: picking
            ? 'phantasi-workbench__note'
            : 'phantasi-workbench__note phantasi-workbench__hover-actions',
          leading: (
            <WorkbenchThumb
              cover
              src={getImageUrl(workbenchNoteCover(doc)) || undefined}
            />
          ),
          selected: notesSelect.selected.has(doc.id),
          onSelect: picking ? () => notesSelect.toggle(doc.id) : undefined,
          renderHit: ({ leading, main }) => (
            <button
              type="button"
              className="managed-list-row-hit"
              disabled={busy}
              onClick={() =>
                picking
                  ? notesSelect.toggle(doc.id)
                  : onOpenNote(workbenchNoteOpen(doc))
              }
            >
              {leading}
              {main}
            </button>
          ),
          actions: picking
            ? []
            : [
                ...(doc.status === 'scheduled'
                  ? [
                      {
                        key: 'unschedule',
                        label: phantasi.workbenchUnschedule,
                        onClick: () => onUnschedule(doc.id, doc.revision),
                        disabled: busy,
                      },
                    ]
                  : []),
                {
                  key: 'delete',
                  label: phantasi.delete,
                  variant: 'danger' as const,
                  confirm: phantasi.workbenchDeleteConfirm,
                  onClick: () => onDeleteNotes([doc]),
                  disabled: busy,
                },
              ],
        }
      }),
    [
      busy,
      locale,
      notesSelect.selecting,
      notesSelect.selected,
      notesSelect.toggle,
      onDeleteNotes,
      onOpenNote,
      onUnschedule,
      phantasi,
      visibleNotes,
    ],
  )

  if (!active) return null

  return (
    <WorkbenchPage
      title={phantasi.workbenchNotes}
      back={back}
      search={
        <WorkbenchSearch
          value={noteQuery}
          onChange={setNoteQuery}
          placeholder={phantasi.workbenchSearchNotes}
        />
      }
      action={
        <>
          <PageAction
            label={phantasi.noteWrite}
            description={phantasi.workbenchWriteHint}
            icon={<LuNotebookPen />}
            primary
            disabled={busy}
            onPick={onWrite}
          />
          <PageAction
            label={phantasi.workbenchCategories}
            description={phantasi.workbenchNoteCategoriesHint}
            icon={<LuTag />}
            onPick={onOpenCategories}
          />
        </>
      }
    >
      <ManagedList
        stats={notesSelectBar.stats}
        toolbar={notesSelectBar.toolbar}
        toolbarPlacement="filters"
        toolbarExtra={
          notesSelect.selecting ? (
            <BatchCategoryPick
              names={noteCategories}
              disabled={busy}
              placeholder={phantasi.workbenchAssignCategory}
              searchPlaceholder={phantasi.workbenchSearchCategories}
              emptyText={phantasi.workbenchCategoryKindEmpty}
              onPick={(name) => {
                const picked = visibleNotes.filter((doc) =>
                  notesSelect.selected.has(doc.id),
                )
                if (picked.length === 0) return
                onAssignNotes?.(picked, name)
              }}
            />
          ) : null
        }
        filterGroups={[
          ...(noteStatuses.length > 0
            ? [
                {
                  label: phantasi.workbenchNoteStatus,
                  icon: <LuClipboardList />,
                  ariaLabel: phantasi.workbenchNoteStatus,
                  options: [
                    { key: 'all', label: phantasi.workbenchMediaAll },
                    ...noteStatuses.map((key) => ({
                      key,
                      label:
                        key === 'scheduled'
                          ? phantasi.noteStatusScheduled
                          : key === 'published'
                            ? phantasi.noteStatusPublished
                            : phantasi.noteStatusDraft,
                    })),
                  ],
                  value: noteStatus,
                  onChange: (key: string) =>
                    setNoteStatus(key as WorkbenchNoteStatusFilter),
                },
              ]
            : []),
          ...(noteTopics.length > 0
            ? [
                {
                  label: phantasi.noteTopic,
                  icon: <LuTag />,
                  ariaLabel: phantasi.noteTopic,
                  options: noteTopicOptions,
                  value: resolvedNoteTopic,
                  onChange: setNoteTopic,
                },
              ]
            : []),
          ...(noteAuthors.length > 0
            ? [
                {
                  label: phantasi.workbenchNoteAuthor,
                  icon: <LuUser />,
                  ariaLabel: phantasi.workbenchNoteAuthor,
                  options: noteAuthorOptions,
                  value: resolvedNoteAuthor,
                  onChange: setNoteAuthor,
                },
              ]
            : []),
        ]}
        queryCollapsible={false}
        queryChrome="plain"
        loading={notesLoading && docs.length === 0}
        working={busy}
        items={noteItems}
        emptyText={
          docs.length === 0
            ? phantasi.workbenchNoteEmpty
            : phantasi.workbenchNoteKindEmpty
        }
        maxHeight={null}
      />
    </WorkbenchPage>
  )
}
