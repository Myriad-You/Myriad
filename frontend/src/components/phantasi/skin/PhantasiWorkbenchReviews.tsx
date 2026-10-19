import type { ReactNode } from 'react'
import type { PhantasiSourceApplication } from '../../../types/phantasi'
import type { ManagedListItem } from '../../settings/ManagedList'
import { useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { ManagedList } from '../../settings'
import { SegmentedControl } from '../../settings/items/ChoiceControls'
import { filterWorkbenchReviews } from '../logic/workbench'
import { noteScheduleLabel } from '../notes/noteBoard'
import { listSelectChrome, useListSelection } from '../useListSelection'
import { WorkbenchPage, WorkbenchSearch } from './PhantasiWorkbenchChrome'

type ReviewStatusFilter = 'pending' | 'approved' | 'rejected' | 'all'

export function WorkbenchReviewsPane({
  active,
  back,
  applications,
  applicationsLoading,
  busy,
  onApprove,
  onReject,
  onDelete,
}: {
  active: boolean
  back: ReactNode
  applications: PhantasiSourceApplication[]
  applicationsLoading: boolean
  busy: boolean
  onApprove?: (id: number) => void
  onReject?: (id: number) => void
  onDelete?: (ids: number[]) => void
}) {
  const { t, locale, format } = useI18n()
  const phantasi = t.phantasi
  const [query, setQuery] = useState('')
  const [status, setStatus] = useState<ReviewStatusFilter>('pending')
  const visible = useMemo(
    () => filterWorkbenchReviews(applications, query, status),
    [applications, query, status],
  )
  const ids = useMemo(() => visible.map((row) => row.id), [visible])
  const select = useListSelection(ids)
  useEffect(() => {
    if (!active) select.exit()
  }, [active, select.exit])

  const selectBar = listSelectChrome({
    selecting: select.selecting,
    picked: select.picked,
    total: select.total,
    allOn: select.allOn,
    busy,
    labels: {
      edit: phantasi.edit,
      selectAll: phantasi.selectAll,
      deselectAll: phantasi.deselectAll,
      deleteSelected: phantasi.deleteSelected,
      deleteConfirm: format(phantasi.workbenchDeleteSelectedReviewsConfirm, {
        count: select.picked,
      }),
      exitEdit: phantasi.exitEdit,
      selectedLabel: phantasi.editMode,
    },
    onEnter: select.enter,
    onExit: select.exit,
    onSelectAll: select.selectAll,
    onDelete: () => {
      const picked = visible
        .filter((row) => select.selected.has(row.id))
        .map((row) => row.id)
      if (picked.length === 0) return
      select.exit()
      onDelete?.(picked)
    },
  })

  const items = useMemo<ManagedListItem[]>(
    () =>
      visible.map((row) => {
        const picking = select.selecting
        const when = noteScheduleLabel(row.created_at, locale)
        const who =
          row.applicant_name?.trim() ||
          row.applicant_email?.trim() ||
          phantasi.anonymousUser
        const kind = row.has_feed
          ? phantasi.workbenchReviewHasFeed
          : phantasi.workbenchReviewLinkOnly
        const state =
          row.status === 'approved'
            ? phantasi.workbenchReviewApproved
            : row.status === 'rejected'
              ? phantasi.workbenchReviewRejected
              : phantasi.workbenchReviewPending
        return {
          id: row.id,
          title: row.site_name,
          subtitle: [row.site_url, who, when].filter(Boolean).join(' · '),
          meta: [kind, state, row.feed_url?.trim() || row.message?.trim()]
            .filter(Boolean)
            .join(' · '),
          selected: select.selected.has(row.id),
          onSelect: picking ? () => select.toggle(row.id) : undefined,
          onClick: picking
            ? () => select.toggle(row.id)
            : () => window.open(row.site_url, '_blank', 'noopener,noreferrer'),
          actions: picking
            ? []
            : [
                {
                  key: 'open',
                  label: phantasi.workbenchReviewOpenSite,
                  onClick: () =>
                    window.open(row.site_url, '_blank', 'noopener,noreferrer'),
                },
                ...(row.status === 'pending'
                  ? [
                      {
                        key: 'approve',
                        label: phantasi.workbenchReviewApprove,
                        confirm: phantasi.workbenchReviewApproveConfirm,
                        onClick: () => onApprove?.(row.id),
                        disabled: busy,
                      },
                      {
                        key: 'reject',
                        label: phantasi.workbenchReviewReject,
                        variant: 'danger' as const,
                        confirm: phantasi.workbenchReviewRejectConfirm,
                        onClick: () => onReject?.(row.id),
                        disabled: busy,
                      },
                    ]
                  : []),
                {
                  key: 'delete',
                  label: phantasi.delete,
                  variant: 'danger' as const,
                  confirm: phantasi.workbenchReviewDeleteConfirm,
                  onClick: () => onDelete?.([row.id]),
                  disabled: busy,
                },
              ],
        }
      }),
    [
      busy,
      locale,
      onApprove,
      onDelete,
      onReject,
      phantasi,
      select.selecting,
      select.selected,
      select.toggle,
      visible,
    ],
  )

  if (!active) return null

  return (
    <WorkbenchPage
      title={phantasi.workbenchReviews}
      back={back}
      search={
        <WorkbenchSearch
          value={query}
          onChange={setQuery}
          placeholder={phantasi.workbenchSearchReviews}
        />
      }
    >
      <SegmentedControl
        ariaLabel={phantasi.workbenchReviews}
        size="sm"
        value={status}
        onChange={setStatus}
        options={[
          { value: 'pending', label: phantasi.workbenchReviewPending },
          { value: 'approved', label: phantasi.workbenchReviewApproved },
          { value: 'rejected', label: phantasi.workbenchReviewRejected },
          { value: 'all', label: phantasi.all },
        ]}
      />
      <ManagedList
        stats={selectBar.stats}
        toolbar={selectBar.toolbar}
        toolbarPlacement="filters"
        queryCollapsible={false}
        queryChrome="plain"
        loading={applicationsLoading && applications.length === 0}
        working={busy}
        items={items}
        emptyText={
          applications.length === 0
            ? phantasi.workbenchReviewEmpty
            : phantasi.workbenchReviewKindEmpty
        }
        maxHeight={null}
      />
    </WorkbenchPage>
  )
}
