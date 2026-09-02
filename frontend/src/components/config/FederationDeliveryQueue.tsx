/**
 * Outbound ActivityPub delivery queue panel for federation settings.
 *
 * Mental model (deliberately small):
 *   - **Retry** — re-queue a failed (non-user-cancel) row, or bulk “retry failures”
 *   - **Remove** — stop + drop from the list (pending → cancel+dismiss; dead → dismiss)
 *   - **Clear** — cancel everything in-flight, then purge all dead (failed + cancelled)
 *
 * Optimistic updates first; quiet refresh only applies successful server payloads.
 */

import type {
  DeliveryQueueItem,
  DeliveryStats,
} from '../../types/federation'
import type {
  ManagedListAction,
  ManagedListFilterOption,
  ManagedListItem,
  ManagedListStat,
  ManagedListTone,
} from '../settings/ManagedList'
import React, { useCallback, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { FaRedo, FaSearch, FaSyncAlt, FaTrash } from '../../lib/icons'
import { federationApi } from '../../services/federationApi'
import {
  isCancelledDeliveryError,
  shouldOfferDeliveryRetry,
} from '../../utils/federationDeliveryUi'
import { userFacingError } from '../../utils/userFacingError'
import { ManagedList } from '../settings/ManagedList'

type Msg = (
  msg: string,
  type?: 'success' | 'error' | 'warning' | 'info',
) => void

function emptyStats(): DeliveryStats {
  return {
    pending: 0,
    delivering: 0,
    delivered: 0,
    dead: 0,
    active: 0,
    failed: 0,
  }
}

function recomputeDerived(s: DeliveryStats): DeliveryStats {
  return {
    ...s,
    active: (s.pending ?? 0) + (s.delivering ?? 0),
    failed: s.dead ?? 0,
  }
}

function patchStats(
  stats: DeliveryStats | null,
  fromStatus: string,
  toStatus: string | null,
): DeliveryStats {
  const next = { ...(stats ?? emptyStats()) }
  const dec = (k: 'pending' | 'delivering' | 'delivered' | 'dead') => {
    next[k] = Math.max(0, (next[k] ?? 0) - 1)
  }
  const inc = (k: 'pending' | 'delivering' | 'delivered' | 'dead') => {
    next[k] = (next[k] ?? 0) + 1
  }
  if (fromStatus === 'pending') dec('pending')
  else if (fromStatus === 'delivering') dec('delivering')
  else if (fromStatus === 'delivered') dec('delivered')
  else if (fromStatus === 'dead' || fromStatus === 'failed') dec('dead')

  if (toStatus === 'pending') inc('pending')
  else if (toStatus === 'delivering') inc('delivering')
  else if (toStatus === 'delivered') inc('delivered')
  else if (toStatus === 'dead') inc('dead')

  return recomputeDerived(next)
}

function statsFromItems(
  items: DeliveryQueueItem[],
  prev: DeliveryStats | null,
): DeliveryStats {
  const next = emptyStats()
  next.delivered = prev?.delivered ?? 0
  for (const it of items) {
    if (it.status === 'pending') next.pending++
    else if (it.status === 'delivering') next.delivering++
    else if (it.status === 'delivered') next.delivered++
    else if (it.status === 'dead' || it.status === 'failed') next.dead++
  }
  return recomputeDerived(next)
}

function isCancelledItem(item: DeliveryQueueItem): boolean {
  return (
    item.intentional_cancel === true ||
    isCancelledDeliveryError(item.error_message)
  )
}

function isActiveItem(item: DeliveryQueueItem): boolean {
  return item.status === 'pending' || item.status === 'delivering'
}

function isTerminalItem(item: DeliveryQueueItem): boolean {
  return item.status === 'dead' || item.status === 'failed'
}

function asPendingRetry(item: DeliveryQueueItem): DeliveryQueueItem {
  return {
    ...item,
    status: 'pending',
    intentional_cancel: false,
    retryable: false,
    error_message: null,
  }
}

function statusTone(
  item: DeliveryQueueItem,
  cancelled: boolean,
): ManagedListTone {
  if (item.status === 'pending') return 'active'
  if (item.status === 'delivering') return 'warn'
  if (item.status === 'delivered') return 'success'
  if (item.status === 'dead' && cancelled) return 'muted'
  if (item.status === 'dead' || item.status === 'failed') return 'danger'
  return 'default'
}

/**
 * Fully drop a row server-side:
 * - active → cancel (marks dead) then dismiss
 * - already terminal → dismiss only
 */
async function removeDeliveryOnServer(item: DeliveryQueueItem): Promise<void> {
  if (isActiveItem(item)) {
    await federationApi.cancelDelivery(item.id)
  }
  if (isActiveItem(item) || isTerminalItem(item)) {
    // After cancel, row is dead; dismiss always for terminal / just-cancelled.
    try {
      await federationApi.dismissDelivery(item.id)
    } catch (e) {
      // Cancel may race worker (row already gone / not dead yet). Retry dismiss once.
      if (isActiveItem(item)) {
        await federationApi.dismissDelivery(item.id)
      } else {
        throw e
      }
    }
  }
}

/**
 * Clear the whole “work queue”: stop in-flight, then hard-delete all dead
 * (failed + cancelled). Does not touch delivered history counters on the server.
 */
async function clearQueueOnServer(): Promise<void> {
  await federationApi.cancelAllPendingDelivery()
  await federationApi.purgeDeadDelivery({ cancelledOnly: false })
}

export interface FederationDeliveryQueueProps {
  stats: DeliveryStats | null
  items: DeliveryQueueItem[]
  onStatsChange: (s: DeliveryStats | null) => void
  onItemsChange: (items: DeliveryQueueItem[]) => void
  onMessage?: Msg
  /** Optional status scopes server query (delivered tab needs this). */
  onRefresh: (status?: string) => Promise<void>
  className?: string
}

export const FederationDeliveryQueue: React.FC<
  FederationDeliveryQueueProps
> = ({
  stats,
  items,
  onStatsChange,
  onItemsChange,
  onMessage,
  onRefresh,
  className,
}) => {
  const { t } = useI18n()
  const c = t.config

  const [bulkBusy, setBulkBusy] = useState(false)
  const [rowBusy, setRowBusy] = useState<Record<number, string | null>>({})
  const [refreshing, setRefreshing] = useState(false)
  const [search, setSearch] = useState('')
  /** all | pending | delivering | delivered | dead */
  const [statusFilter, setStatusFilter] = useState('all')

  const anyBusy = bulkBusy || refreshing

  const setItemBusy = useCallback((id: number, action: string | null) => {
    setRowBusy((prev) => {
      if (action == null) {
        const next = { ...prev }
        delete next[id]
        return next
      }
      return { ...prev, [id]: action }
    })
  }, [])

  const quietRefresh = useCallback(async () => {
    try {
      await onRefresh(statusFilter === 'all' ? undefined : statusFilter)
    } catch {
      /* keep optimistic state */
    }
  }, [onRefresh, statusFilter])

  // Server-side status filter when tab changes (default list prioritizes dead)
  React.useEffect(() => {
    void onRefresh(statusFilter === 'all' ? undefined : statusFilter)
  }, [statusFilter, onRefresh])

  const fail = useCallback(
    (e: unknown) => {
      onMessage?.(
        userFacingError(e, c.federationDeliveryActionFailed),
        'error',
      )
    },
    [onMessage, c.federationDeliveryActionFailed],
  )

  const handleRefresh = useCallback(async () => {
    setRefreshing(true)
    try {
      // Keep status filter (tab change path already passes it)
      await onRefresh(statusFilter === 'all' ? undefined : statusFilter)
    } catch (e) {
      fail(e)
    } finally {
      setRefreshing(false)
    }
  }, [onRefresh, fail, statusFilter])

  const handleRetry = useCallback(
    async (item: DeliveryQueueItem) => {
      const snapshot = { items: [...items], stats }
      setItemBusy(item.id, 'retry')
      const nextItems = items.map((it) =>
        it.id === item.id ? asPendingRetry(it) : it,
      )
      onItemsChange(nextItems)
      onStatsChange(patchStats(stats, item.status, 'pending'))
      try {
        await federationApi.retryDelivery(item.id)
        setItemBusy(item.id, null)
        await quietRefresh()
      } catch (e) {
        onItemsChange(snapshot.items)
        onStatsChange(snapshot.stats)
        fail(e)
        setItemBusy(item.id, null)
      }
    },
    [items, stats, setItemBusy, onItemsChange, onStatsChange, quietRefresh, fail],
  )

  /** Unified remove: cancel if active, always drop terminal rows from the list. */
  const handleRemove = useCallback(
    async (item: DeliveryQueueItem) => {
      const snapshot = { items: [...items], stats }
      setItemBusy(item.id, 'remove')
      const nextItems = items.filter((it) => it.id !== item.id)
      onItemsChange(nextItems)
      onStatsChange(patchStats(stats, item.status, null))
      try {
        await removeDeliveryOnServer(item)
        setItemBusy(item.id, null)
        await quietRefresh()
      } catch (e) {
        onItemsChange(snapshot.items)
        onStatsChange(snapshot.stats)
        fail(e)
        setItemBusy(item.id, null)
      }
    },
    [items, stats, setItemBusy, onItemsChange, onStatsChange, quietRefresh, fail],
  )

  const handleRetryFailures = useCallback(async () => {
    const snapshot = { items: [...items], stats }
    setBulkBusy(true)
    const nextItems = items.map((it) =>
      isTerminalItem(it) && shouldOfferDeliveryRetry(it)
        ? asPendingRetry(it)
        : it,
    )
    onItemsChange(nextItems)
    onStatsChange(statsFromItems(nextItems, stats))
    try {
      await federationApi.retryAllDeadDelivery()
      await quietRefresh()
    } catch (e) {
      onItemsChange(snapshot.items)
      onStatsChange(snapshot.stats)
      fail(e)
    } finally {
      setBulkBusy(false)
    }
  }, [items, stats, onItemsChange, onStatsChange, quietRefresh, fail])

  /**
   * Clear: cancel all in-flight + purge every dead row (failed and cancelled).
   * Leaves only rows that are still delivering mid-flight race / delivered stats.
   */
  const handleClear = useCallback(async () => {
    const snapshot = { items: [...items], stats }
    setBulkBusy(true)
    // Keep nothing terminal or active in the optimistic list.
    const nextItems = items.filter(
      (it) => !isActiveItem(it) && !isTerminalItem(it),
    )
    onItemsChange(nextItems)
    onStatsChange(statsFromItems(nextItems, stats))
    try {
      await clearQueueOnServer()
      await quietRefresh()
    } catch (e) {
      onItemsChange(snapshot.items)
      onStatsChange(snapshot.stats)
      fail(e)
    } finally {
      setBulkBusy(false)
    }
  }, [items, stats, onItemsChange, onStatsChange, quietRefresh, fail])

  const hasFailures = useMemo(
    () => items.some((it) => isTerminalItem(it) && shouldOfferDeliveryRetry(it)),
    [items],
  )
  const hasClearable = useMemo(
    () => items.some((it) => isActiveItem(it) || isTerminalItem(it)),
    [items],
  )

  const listStats = useMemo((): ManagedListStat[] => {
    const s = stats ?? emptyStats()
    return [
      {
        key: 'pending',
        label: c.federationDeliveryStatPending,
        value: s.pending ?? 0,
        tone: (s.pending ?? 0) > 0 ? 'active' : 'muted',
      },
      {
        key: 'delivering',
        label: c.federationDeliveryStatDelivering,
        value: s.delivering ?? 0,
        tone: (s.delivering ?? 0) > 0 ? 'warn' : 'muted',
      },
      {
        key: 'delivered',
        label: c.federationDeliveryStatDelivered,
        value: s.delivered ?? 0,
        tone: 'success',
      },
      {
        key: 'dead',
        label: c.federationDeliveryStatDead,
        value: s.dead ?? 0,
        tone: (s.dead ?? 0) > 0 ? 'danger' : 'muted',
      },
    ]
  }, [stats, c])

  const toolbar = useMemo((): ManagedListAction[] => {
    return [
      {
        key: 'refresh',
        label: c.federationDeliveryRefresh,
        description: c.federationDeliveryRefreshDesc,
        icon: <FaSyncAlt aria-hidden />,
        onClick: () => void handleRefresh(),
        disabled: anyBusy,
        loading: refreshing,
        variant: 'secondary',
      },
      {
        key: 'retry-failures',
        label: c.federationDeliveryRetryFailures,
        description: c.federationDeliveryRetryFailuresDesc,
        icon: <FaRedo aria-hidden />,
        onClick: () => void handleRetryFailures(),
        disabled: anyBusy || !hasFailures,
        loading: bulkBusy,
        variant: 'primary',
        confirm: c.federationDeliveryRetryFailuresConfirm,
      },
      {
        key: 'clear',
        label: c.federationDeliveryClear,
        description: c.federationDeliveryClearDesc,
        icon: <FaTrash aria-hidden />,
        onClick: () => void handleClear(),
        disabled: anyBusy || !hasClearable,
        loading: bulkBusy,
        variant: 'danger',
        confirm: c.federationDeliveryClearConfirm,
      },
    ]
  }, [
    c,
    anyBusy,
    refreshing,
    bulkBusy,
    hasFailures,
    hasClearable,
    handleRefresh,
    handleRetryFailures,
    handleClear,
  ])

  const statusFilterOptions = useMemo((): ManagedListFilterOption[] => {
    const counts = {
      all: items.length,
      pending: 0,
      delivering: 0,
      delivered: 0,
      dead: 0,
    }
    for (const it of items) {
      if (it.status === 'pending') counts.pending++
      else if (it.status === 'delivering') counts.delivering++
      else if (it.status === 'delivered') counts.delivered++
      else if (it.status === 'dead' || it.status === 'failed') counts.dead++
    }
    return [
      {
        key: 'all',
        label: c.federationDeliveryFilterAll,
        count: counts.all,
      },
      {
        key: 'pending',
        label: c.federationDeliveryStatusPending,
        count: counts.pending,
      },
      {
        key: 'delivering',
        label: c.federationDeliveryStatusDelivering,
        count: counts.delivering,
      },
      {
        key: 'delivered',
        label: c.federationDeliveryStatusDelivered,
        count: counts.delivered,
      },
      {
        key: 'dead',
        label: c.federationDeliveryStatDead,
        count: counts.dead,
      },
    ]
  }, [items, c])

  const filteredItems = useMemo(() => {
    const q = search.trim().toLowerCase()
    return items.filter((item) => {
      if (statusFilter === 'pending' && item.status !== 'pending') return false
      if (statusFilter === 'delivering' && item.status !== 'delivering')
        return false
      if (statusFilter === 'delivered' && item.status !== 'delivered')
        return false
      if (
        statusFilter === 'dead' &&
        item.status !== 'dead' &&
        item.status !== 'failed'
      ) {
        return false
      }
      if (!q) return true
      const hay = [
        String(item.id),
        item.activity_type,
        item.target_domain,
        item.target_inbox,
        item.error_message,
        item.status,
      ]
        .filter(Boolean)
        .join(' ')
        .toLowerCase()
      return hay.includes(q)
    })
  }, [items, search, statusFilter])

  const queryActive = search.trim().length > 0 || statusFilter !== 'all'

  const emptyText =
    items.length === 0
      ? c.federationDeliveryEmpty
      : c.federationDeliveryFilterEmpty

  /** Soft render cap — keep in sync with ManagedList maxVisibleItems below. */
  const LIST_CAP = 60
  const listTruncated = filteredItems.length > LIST_CAP

  // When truncated, ManagedList.truncateFooter owns the count line (avoid double footer).
  const footer =
    queryActive && items.length > 0 && !listTruncated
      ? c.federationDeliveryShowing
          .replace('{shown}', String(filteredItems.length))
          .replace('{total}', String(items.length))
      : undefined

  const listItems = useMemo((): ManagedListItem[] => {
    return filteredItems.map((item) => {
      const cancelled = isCancelledItem(item)
      const statusLabel =
        item.status === 'dead' && cancelled
          ? c.federationDeliveryStatusCancelled
          : item.status === 'dead' || item.status === 'failed'
            ? c.federationDeliveryStatusFailed
            : item.status === 'pending'
              ? c.federationDeliveryStatusPending
              : item.status === 'delivering'
                ? c.federationDeliveryStatusDelivering
                : item.status === 'delivered'
                  ? c.federationDeliveryStatusDelivered
                  : item.status
      const attemptsLabel = c.federationDeliveryAttempts
        .replace('{attempts}', String(item.attempts ?? 0))
        .replace('{max}', String(item.max_attempts ?? 0))
      const target = item.target_domain || item.target_inbox || '—'
      const busyAction = rowBusy[item.id]
      const showRetry = shouldOfferDeliveryRetry(item)
      // Unified remove: any non-delivered row can leave the queue.
      const showRemove = isActiveItem(item) || isTerminalItem(item)

      const actions: ManagedListAction[] = []
      if (showRetry) {
        actions.push({
          key: 'retry',
          label: c.federationDeliveryRetry,
          onClick: () => void handleRetry(item),
          disabled: anyBusy || !!busyAction,
          loading: busyAction === 'retry',
          variant: 'primary',
        })
      }
      if (showRemove) {
        actions.push({
          key: 'remove',
          label: c.federationDeliveryRemove,
          onClick: () => void handleRemove(item),
          disabled: anyBusy || !!busyAction,
          loading: busyAction === 'remove',
          variant: 'danger',
        })
      }

      return {
        id: item.id,
        badge: { label: statusLabel, tone: statusTone(item, cancelled) },
        title: (
          <>
            <span className="managed-list-id">#{item.id}</span>
            {' · '}
            {item.activity_type || '—'}
          </>
        ),
        subtitle: (
          <>
            {target}
            {!cancelled && item.error_message ? (
              <>
                <br />
                <span className="managed-list-error">
                  {userFacingError(
                    item.error_message,
                    t.errors.noticeDeliveryFailed,
                  )}
                </span>
              </>
            ) : null}
          </>
        ),
        meta: attemptsLabel,
        actions,
        busy: !!busyAction,
      }
    })
  }, [
    filteredItems,
    rowBusy,
    anyBusy,
    c,
    handleRetry,
    handleRemove,
    t.errors.noticeDeliveryFailed,
  ])

  return (
    <ManagedList
      className={className}
      stats={listStats}
      toolbar={toolbar}
      queryToggleLabel={c.federationListQueryToggle}
      queryToggleDescription={c.federationListQueryToggleDesc}
      queryToggleIcon={<FaSearch aria-hidden />}
      queryCollapseLabel={c.federationListQueryCollapse}
      queryCollapseDescription={c.federationListQueryCollapseDesc}
      search={{
        value: search,
        onChange: setSearch,
        placeholder: c.federationDeliverySearchPlaceholder,
        ariaLabel: c.federationDeliverySearchAria,
      }}
      filters={{
        options: statusFilterOptions,
        value: statusFilter,
        onChange: setStatusFilter,
        ariaLabel: c.federationDeliveryFilterAria,
      }}
      items={listItems}
      emptyText={emptyText}
      footer={footer}
      working={bulkBusy}
      maxHeight={filteredItems.length > 8 ? '20rem' : null}
      maxVisibleItems={LIST_CAP}
      truncateFooter={(shown, total) =>
        // shown = rendered rows; total = filtered list size (items prop length)
        c.federationDeliveryShowing
          .replace('{shown}', String(shown))
          .replace('{total}', String(total))
      }
    />
  )
}

export default FederationDeliveryQueue
