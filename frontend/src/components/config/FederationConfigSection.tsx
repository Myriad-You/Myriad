import type {
  ContentFilterItem,
  DeliveryQueueItem,
  DeliveryStats,
  FederationIdentity,
  FederationInstance,
  TrustPolicyResponse,
  UpdateTrustPolicyRequest,
} from '../../types/federation'
import type { InfoActionField } from '../settings'
import type {
  ManagedListFilterOption,
  ManagedListItem,
  ManagedListStat,
} from '../settings/ManagedList'
import {
  FaCog,
  FaFilter,
  FaKey,
  FaPaperPlane,
  FaPlus,
  FaSearch,
  FaServer,
  LuShieldCheck,
} from '@lib/icons'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { federationApi } from '../../services/federationApi'
import { userFacingError } from '../../utils/userFacingError'
import {
  AutoHeight,
  FieldSelect,
  guideDomProps,
  InfoActionCard,
  InputItem,
  ManagedList,
  NumberItem,
  SelectItem,
  SettingGroup,
  SettingsButton,
  SettingSection,
  SettingTitleGuideEntry,
  SwitchItem,
  useSettingGuide,
} from '../settings'
import { FederationDeliveryQueue } from './FederationDeliveryQueue'

/** owned by ConfigForm save */
export interface FederationPolicyDraft {
  minTrust: number
  allowlistText: string
  autoDiscover: boolean
  rateMax: number
  rateWindow: number
  rateTrustedMul: number
}

export const DEFAULT_FEDERATION_POLICY: FederationPolicyDraft = {
  minTrust: 0,
  allowlistText: '',
  autoDiscover: true,
  rateMax: 100,
  rateWindow: 60,
  rateTrustedMul: 5,
}

export function federationPolicyFromApi(
  p: TrustPolicyResponse,
): FederationPolicyDraft {
  return {
    minTrust: p.min_trust_level ?? 0,
    allowlistText: (p.allowed_domains || []).join('\n'),
    autoDiscover: p.auto_discover !== false,
    rateMax: p.rate_limit?.max_requests_per_window ?? 100,
    rateWindow: p.rate_limit?.window_seconds ?? 60,
    rateTrustedMul: p.rate_limit?.trusted_multiplier ?? 5,
  }
}

export function areFederationPoliciesEqual(
  a: FederationPolicyDraft,
  b: FederationPolicyDraft,
): boolean {
  return (
    a.minTrust === b.minTrust &&
    a.allowlistText === b.allowlistText &&
    a.autoDiscover === b.autoDiscover &&
    a.rateMax === b.rateMax &&
    a.rateWindow === b.rateWindow &&
    a.rateTrustedMul === b.rateTrustedMul
  )
}

export function federationPolicyToUpdateRequest(
  draft: FederationPolicyDraft,
): UpdateTrustPolicyRequest {
  const domains = draft.allowlistText
    .split(/[\n,]+/)
    .map((s) => s.trim().toLowerCase())
    .filter(Boolean)
  return {
    min_trust_level: draft.minTrust,
    allowed_domains: domains,
    auto_discover: draft.autoDiscover,
    rate_limit: {
      max_requests_per_window: draft.rateMax,
      window_seconds: draft.rateWindow,
      trusted_multiplier: draft.rateTrustedMul,
    },
  }
}

interface FederationConfigSectionProps {
  title: string
  icon: React.ReactNode
  description: string
  sectionId?: string
  policyDraft: FederationPolicyDraft
  onPolicyChange: (patch: Partial<FederationPolicyDraft>) => void
  onMessage?: (
    msg: string,
    type?: 'success' | 'error' | 'warning' | 'info',
  ) => void
}

const FILTER_TYPES = [
  'block_activity_type',
  'block_keyword',
  'require_trust_level',
] as const

type FilterType = (typeof FILTER_TYPES)[number]

const ACTIVITY_TYPES = [
  'Follow',
  'Accept',
  'Reject',
  'Undo',
  'Create',
  'Update',
  'Delete',
  'Announce',
  'Like',
  'Move',
  'myriad:ChannelOpen',
  'myriad:ChannelClose',
  'myriad:ChannelAccept',
  'myriad:ChannelMessage',
  'myriad:RoomInvite',
  'myriad:RoomJoin',
  'myriad:RoomLeave',
  'myriad:RoomDissolve',
  'myriad:RoomMessage',
  'myriad:RoomPin',
  'myriad:RoomGovernance',
  'myriad:RingJoin',
  'myriad:RingSync',
  'myriad:RingLeave',
  'myriad:FileTransfer',
  'myriad:KeyExchange',
] as const

function activityTypeFallbackLabel(type: string): string {
  if (type.startsWith('myriad:')) {
    return `${type.slice('myriad:'.length)} (MFP)`
  }
  return type
}

function defaultValueForFilterType(type: FilterType): string {
  switch (type) {
    case 'block_activity_type':
      return 'Announce'
    case 'require_trust_level':
      return '0'
    case 'block_keyword':
    default:
      return ''
  }
}

export const FederationConfigSection: React.FC<
  FederationConfigSectionProps
> = ({
  title,
  icon,
  description,
  sectionId,
  policyDraft,
  onPolicyChange,
  onMessage,
}) => {
  const { t, format } = useI18n()
  const c = t.config
  const { catalog: g, bindGuide } = useSettingGuide()
  const [instances, setInstances] = useState<FederationInstance[]>([])
  const [filters, setFilters] = useState<ContentFilterItem[]>([])
  const [identity, setIdentity] = useState<FederationIdentity | null>(null)
  const [rotatingKeys, setRotatingKeys] = useState(false)
  const [deliveryStats, setDeliveryStats] = useState<DeliveryStats | null>(null)
  const [deliveryItems, setDeliveryItems] = useState<DeliveryQueueItem[]>([])

  const [newFilterName, setNewFilterName] = useState('')
  const [newFilterType, setNewFilterType] =
    useState<FilterType>('block_keyword')
  const [newFilterValue, setNewFilterValue] = useState('')
  const [filterFormOpen, setFilterFormOpen] = useState(false)
  const [instanceBusy, setInstanceBusy] = useState<Record<string, boolean>>({})
  const [instanceSearch, setInstanceSearch] = useState('')
  const [instanceFilter, setInstanceFilter] = useState('all')
  const [contentFilterSearch, setContentFilterSearch] = useState('')
  const [contentFilterChip, setContentFilterChip] = useState('all')
  const [filterBusy, setFilterBusy] = useState<
    Record<number, 'toggle' | 'delete' | undefined>
  >({})

  const trustLevels = useMemo(
    () => [
      { value: 0, label: c.federationTrustUnknown },
      { value: 1, label: c.federationTrustDiscovered },
      { value: 2, label: c.federationTrustFollowed },
      { value: 3, label: c.federationTrustTrusted },
      { value: 4, label: c.federationTrustFederated },
    ],
    [c],
  )

  const trustFilterLabels = useMemo(
    () => [
      { value: 0, label: c.federationInstanceFilterLevel0 },
      { value: 1, label: c.federationInstanceFilterLevel1 },
      { value: 2, label: c.federationInstanceFilterLevel2 },
      { value: 3, label: c.federationInstanceFilterLevel3 },
      { value: 4, label: c.federationInstanceFilterLevel4 },
    ],
    [c],
  )

  const trustLevelOptions = useMemo(
    () =>
      trustLevels.map((l) => ({
        value: String(l.value),
        label: l.label,
      })),
    [trustLevels],
  )

  const filterTypeLabels: Record<FilterType, string> = useMemo(
    () => ({
      block_activity_type: c.federationFilterTypeBlockActivity,
      block_keyword: c.federationFilterTypeBlockKeyword,
      require_trust_level: c.federationFilterTypeRequireTrust,
    }),
    [c],
  )

  const filterTypeOptions = useMemo(
    () =>
      FILTER_TYPES.map((ft) => ({
        value: ft,
        label: filterTypeLabels[ft],
      })),
    [filterTypeLabels],
  )

  const activityTypeLabels = useMemo((): Record<string, string> => {
    return {
      Follow: c.federationActivityFollow,
      Accept: c.federationActivityAccept,
      Reject: c.federationActivityReject,
      Undo: c.federationActivityUndo,
      Create: c.federationActivityCreate,
      Update: c.federationActivityUpdate,
      Delete: c.federationActivityDelete,
      Announce: c.federationActivityAnnounce,
      Like: c.federationActivityLike,
      Move: c.federationActivityMove,
      'myriad:ChannelOpen': c.federationActivityChannelOpen,
      'myriad:ChannelClose': c.federationActivityChannelClose,
      'myriad:ChannelAccept': c.federationActivityChannelAccept,
      'myriad:ChannelMessage': c.federationActivityChannelMessage,
      'myriad:RoomInvite': c.federationActivityRoomInvite,
      'myriad:RoomJoin': c.federationActivityRoomJoin,
      'myriad:RoomLeave': c.federationActivityRoomLeave,
      'myriad:RoomDissolve': c.federationActivityRoomDissolve,
      'myriad:RoomMessage': c.federationActivityRoomMessage,
      'myriad:RoomPin': c.federationActivityRoomPin,
      'myriad:RoomGovernance': c.federationActivityRoomGovernance,
      'myriad:RingJoin': c.federationActivityRingJoin,
      'myriad:RingSync': c.federationActivityRingSync,
      'myriad:RingLeave': c.federationActivityRingLeave,
      'myriad:FileTransfer': c.federationActivityFileTransfer,
      'myriad:KeyExchange': c.federationActivityKeyExchange,
    }
  }, [c])

  const resolveActivityTypeLabel = useCallback(
    (type: string): string =>
      activityTypeLabels[type] ?? activityTypeFallbackLabel(type),
    [activityTypeLabels],
  )

  const activityTypeOptions = useMemo(
    () =>
      ACTIVITY_TYPES.map((ty) => ({
        value: ty,
        label: resolveActivityTypeLabel(ty),
      })),
    [resolveActivityTypeLabel],
  )

  const filterTypeHelp = useMemo((): Record<FilterType, string> => {
    return {
      block_activity_type: c.federationFilterDescBlockActivity,
      block_keyword: c.federationFilterDescBlockKeyword,
      require_trust_level: c.federationFilterDescRequireTrust,
    }
  }, [c])

  /** apply successful responses only; never wipe a good list on a blip */
  const loadDelivery = useCallback(async (status?: string) => {
    const statusParam =
      status && status !== 'all' ? status : undefined
    const [statsResult, listResult] = await Promise.allSettled([
      federationApi.getDeliveryStats(),
      federationApi.listDelivery(25, undefined, statusParam),
    ])
    if (statsResult.status === 'fulfilled' && statsResult.value) {
      setDeliveryStats(statsResult.value)
    }
    if (listResult.status === 'fulfilled' && listResult.value) {
      setDeliveryItems(listResult.value.items || [])
    }
    // first paint: leave empty; do not wipe
  }, [])

  const load = useCallback(async () => {
    try {
      const [inst, f, id] = await Promise.all([
        federationApi.getInstances().catch(() => ({ instances: [], total: 0 })),
        federationApi
          .listContentFilters()
          .catch(() => ({ filters: [], total: 0 })),
        federationApi.getIdentity().catch(() => null),
      ])
      setInstances(inst.instances || [])
      setFilters(f.filters || [])
      setIdentity(id)
      await loadDelivery()
    } catch (e) {
      onMessage?.(userFacingError(e, c.federationLoadFailed), 'error')
    }
  }, [onMessage, c.federationLoadFailed, loadDelivery])

  useEffect(() => {
    void load()
  }, [load])

  const rotateKeys = async () => {
    setRotatingKeys(true)
    try {
      await federationApi.rotateKeys({ confirm: true })
      onMessage?.(c.federationKeysRotateSuccess, 'success')
      const id = await federationApi.getIdentity().catch(() => null)
      setIdentity(id)
    } catch (e) {
      onMessage?.(userFacingError(e, c.federationKeysRotateFailed), 'error')
    } finally {
      setRotatingKeys(false)
    }
  }

  const setInstanceTrust = useCallback(
    async (domain: string, level: number) => {
      try {
        await federationApi.updateInstanceTrust({ domain, trust_level: level })
        setInstances((prev) =>
          prev.map((i) =>
            i.domain === domain ? { ...i, trust_level: level } : i,
          ),
        )
      } catch (e) {
        onMessage?.(userFacingError(e, c.federationUpdateFailed), 'error')
      }
    },
    [onMessage, c.federationUpdateFailed],
  )

  const toggleBlock = useCallback(
    async (domain: string, block: boolean) => {
      let snapshot: FederationInstance[] = []
      setInstanceBusy((b) => ({ ...b, [domain]: true }))
      setInstances((prev) => {
        snapshot = prev
        return prev.map((i) =>
          i.domain === domain ? { ...i, blocked: block } : i,
        )
      })
      try {
        await federationApi.toggleInstanceBlock({ domain, block })
      } catch (e) {
        setInstances(snapshot)
        onMessage?.(userFacingError(e, c.federationBlockFailed), 'error')
      } finally {
        setInstanceBusy((b) => {
          const next = { ...b }
          delete next[domain]
          return next
        })
      }
    },
    [onMessage, c.federationBlockFailed],
  )

  const handleFilterTypeChange = (next: string) => {
    const type = next as FilterType
    setNewFilterType(type)
    setNewFilterValue(defaultValueForFilterType(type))
  }

  const formatFilterValue = useCallback(
    (filterType: string, value: string): string => {
      if (filterType === 'block_activity_type') {
        return resolveActivityTypeLabel(value)
      }
      if (filterType === 'require_trust_level') {
        const n = Number.parseInt(value, 10)
        const level = trustLevels.find((l) => l.value === n)
        return level?.label ?? value
      }
      return value
    },
    [trustLevels, resolveActivityTypeLabel],
  )

  const addFilter = async () => {
    if (!newFilterName.trim() || !newFilterValue.trim()) {
      onMessage?.(c.federationFilterNameValueRequired, 'warning')
      return
    }
    try {
      await federationApi.createContentFilter({
        name: newFilterName.trim(),
        filter_type: newFilterType,
        value: newFilterValue.trim(),
        enabled: true,
      })
      setNewFilterName('')
      setNewFilterValue(defaultValueForFilterType(newFilterType))
      setFilterFormOpen(false)
      await load()
      onMessage?.(c.federationFilterAdded, 'success')
    } catch (e) {
      onMessage?.(userFacingError(e, c.federationAddFilterFailed), 'error')
    }
  }

  const toggleFilter = async (f: ContentFilterItem) => {
    const snapshot = filters
    setFilterBusy((b) => ({ ...b, [f.id]: 'toggle' }))
    setFilters((prev) =>
      prev.map((x) => (x.id === f.id ? { ...x, enabled: !x.enabled } : x)),
    )
    try {
      await federationApi.updateContentFilter(f.id, { enabled: !f.enabled })
    } catch (e) {
      setFilters(snapshot)
      onMessage?.(userFacingError(e, c.federationUpdateFailed), 'error')
    } finally {
      setFilterBusy((b) => {
        const next = { ...b }
        delete next[f.id]
        return next
      })
    }
  }

  const deleteFilter = async (id: number) => {
    const snapshot = filters
    setFilterBusy((b) => ({ ...b, [id]: 'delete' }))
    setFilters((prev) => prev.filter((x) => x.id !== id))
    try {
      await federationApi.deleteContentFilter(id)
    } catch (e) {
      setFilters(snapshot)
      onMessage?.(userFacingError(e, c.federationUpdateFailed), 'error')
    } finally {
      setFilterBusy((b) => {
        const next = { ...b }
        delete next[id]
        return next
      })
    }
  }

  const instanceStats = useMemo((): ManagedListStat[] => {
    let active = 0
    let blocked = 0
    let trusted = 0
    for (const inst of instances) {
      if (inst.blocked) blocked++
      else active++
      if (!inst.blocked && (inst.trust_level ?? 0) >= 3) trusted++
    }
    return [
      {
        key: 'total',
        label: c.federationInstanceStatTotal,
        value: instances.length,
        tone: instances.length > 0 ? 'active' : 'muted',
      },
      {
        key: 'active',
        label: c.federationInstanceStatActive,
        value: active,
        tone: active > 0 ? 'success' : 'muted',
      },
      {
        key: 'blocked',
        label: c.federationInstanceStatBlocked,
        value: blocked,
        tone: blocked > 0 ? 'danger' : 'muted',
      },
      {
        key: 'trusted',
        label: c.federationInstanceStatTrusted,
        value: trusted,
        tone: trusted > 0 ? 'warn' : 'muted',
      },
    ]
  }, [instances, c])

  const instanceFilterCounts = useMemo(() => {
    const counts: Record<string, number> = {
      all: instances.length,
      active: 0,
      blocked: 0,
      'level:0': 0,
      'level:1': 0,
      'level:2': 0,
      'level:3': 0,
      'level:4': 0,
    }
    for (const inst of instances) {
      if (inst.blocked) counts.blocked++
      else counts.active++
      const level = Math.min(4, Math.max(0, inst.trust_level ?? 0))
      counts[`level:${level}`] = (counts[`level:${level}`] ?? 0) + 1
    }
    return counts
  }, [instances])

  const instanceFilterOptions = useMemo((): ManagedListFilterOption[] => {
    return [
      {
        key: 'all',
        label: c.federationInstanceFilterAll,
        count: instanceFilterCounts.all,
      },
      {
        key: 'active',
        label: c.federationInstanceFilterActive,
        count: instanceFilterCounts.active,
      },
      {
        key: 'blocked',
        label: c.federationInstanceFilterBlocked,
        count: instanceFilterCounts.blocked,
      },
      ...trustFilterLabels.map((l) => ({
        key: `level:${l.value}`,
        label: l.label,
        count: instanceFilterCounts[`level:${l.value}`] ?? 0,
      })),
    ]
  }, [c, instanceFilterCounts, trustFilterLabels])

  const filteredInstances = useMemo(() => {
    const q = instanceSearch.trim().toLowerCase()
    return instances.filter((inst) => {
      if (instanceFilter === 'active' && inst.blocked) return false
      if (instanceFilter === 'blocked' && !inst.blocked) return false
      if (instanceFilter.startsWith('level:')) {
        const level = Number(instanceFilter.slice('level:'.length))
        if ((inst.trust_level ?? 0) !== level) return false
      }
      if (!q) return true
      const hay = [inst.domain, inst.software ?? '', inst.version ?? '']
        .join(' ')
        .toLowerCase()
      return hay.includes(q)
    })
  }, [instances, instanceSearch, instanceFilter])

  const instanceListItems: ManagedListItem[] = useMemo(
    () =>
      filteredInstances.map((inst) => {
        const softwareMeta =
          inst.software || inst.version
            ? [inst.software, inst.version].filter(Boolean).join(' · ')
            : undefined
        return {
          id: inst.domain,
          title: inst.domain,
          subtitle: softwareMeta,
          badge: inst.blocked
            ? { label: c.federationBlocked, tone: 'danger' as const }
            : undefined,
          busy: !!instanceBusy[inst.domain],
          trailing: (
            <FieldSelect
              size="sm"
              value={String(inst.trust_level)}
              options={trustLevels.map((l) => ({
                value: String(l.value),
                label: l.label,
              }))}
              onChange={(v) => void setInstanceTrust(inst.domain, Number(v))}
              aria-label={c.federationInstanceTrustAria}
              disabled={!!instanceBusy[inst.domain]}
            />
          ),
          actions: [
            {
              key: 'block',
              label: inst.blocked ? c.federationUnblock : c.federationBlock,
              variant: (inst.blocked ? 'primary' : 'danger') as
                'primary' | 'danger',
              onClick: () => void toggleBlock(inst.domain, !inst.blocked),
              loading: !!instanceBusy[inst.domain],
            },
          ],
        }
      }),
    [
      filteredInstances,
      instanceBusy,
      trustLevels,
      c.federationBlocked,
      c.federationUnblock,
      c.federationBlock,
      c.federationInstanceTrustAria,
      setInstanceTrust,
      toggleBlock,
    ],
  )

  const instanceQueryActive =
    instanceSearch.trim().length > 0 || instanceFilter !== 'all'

  const instanceEmptyText =
    instances.length === 0
      ? c.federationNoInstances
      : c.federationInstanceFilterEmpty

  const INSTANCE_LIST_CAP = 60
  const instanceListTruncated =
    instances.length > 8 && filteredInstances.length > INSTANCE_LIST_CAP

  const instanceFooter =
    instanceQueryActive && instances.length > 0 && !instanceListTruncated
      ? format(c.federationInstanceShowing, {
          shown: filteredInstances.length,
          total: instances.length,
        })
      : undefined

  const filterStats = useMemo((): ManagedListStat[] => {
    const enabled = filters.filter((f) => f.enabled).length
    const disabled = filters.length - enabled
    return [
      {
        key: 'total',
        label: c.federationInstanceStatTotal,
        value: filters.length,
      },
      {
        key: 'enabled',
        label: c.federationFilterEnabled,
        value: enabled,
        tone: 'success',
      },
      {
        key: 'disabled',
        label: c.federationFilterDisabled,
        value: disabled,
        tone: 'muted',
      },
    ]
  }, [
    filters,
    c.federationInstanceStatTotal,
    c.federationFilterEnabled,
    c.federationFilterDisabled,
  ])

  const contentFilterChipOptions = useMemo((): ManagedListFilterOption[] => {
    const typeCounts: Record<string, number> = {
      block_activity_type: 0,
      block_keyword: 0,
      require_trust_level: 0,
    }
    let enabled = 0
    let disabled = 0
    for (const f of filters) {
      if (f.enabled) enabled++
      else disabled++
      if (Object.hasOwn(typeCounts, f.filter_type)) {
        typeCounts[f.filter_type]++
      }
    }
    return [
      {
        key: 'all',
        label: c.federationInstanceFilterAll,
        count: filters.length,
      },
      {
        key: 'enabled',
        label: c.federationFilterEnabled,
        count: enabled,
      },
      {
        key: 'disabled',
        label: c.federationFilterDisabled,
        count: disabled,
      },
      ...filterTypeOptions.map((opt) => ({
        key: `type:${opt.value}`,
        label: opt.label,
        count: typeCounts[opt.value] ?? 0,
      })),
    ]
  }, [
    filters,
    filterTypeOptions,
    c.federationInstanceFilterAll,
    c.federationFilterEnabled,
    c.federationFilterDisabled,
  ])

  const filteredContentFilters = useMemo(() => {
    const q = contentFilterSearch.trim().toLowerCase()
    return filters.filter((f) => {
      if (contentFilterChip === 'enabled' && !f.enabled) return false
      if (contentFilterChip === 'disabled' && f.enabled) return false
      if (contentFilterChip.startsWith('type:')) {
        const type = contentFilterChip.slice('type:'.length)
        if (f.filter_type !== type) return false
      }
      if (!q) return true
      const typeLabel =
        (filterTypeLabels as Record<string, string>)[f.filter_type] ||
        f.filter_type
      const valueLabel = formatFilterValue(f.filter_type, f.value)
      const hay = [f.name, f.filter_type, typeLabel, f.value, valueLabel]
        .join(' ')
        .toLowerCase()
      return hay.includes(q)
    })
  }, [
    filters,
    contentFilterSearch,
    contentFilterChip,
    filterTypeLabels,
    formatFilterValue,
  ])

  const contentFilterQueryActive =
    contentFilterSearch.trim().length > 0 || contentFilterChip !== 'all'

  const contentFilterEmptyText =
    filters.length === 0 ? c.federationNoFilters : c.federationFilterFilterEmpty

  const contentFilterFooter =
    contentFilterQueryActive && filters.length > 0
      ? format(c.federationFilterShowing, {
          shown: filteredContentFilters.length,
          total: filters.length,
        })
      : undefined

  const filterListItems: ManagedListItem[] = useMemo(
    () =>
      filteredContentFilters.map((f) => {
        const typeLabel =
          (filterTypeLabels as Record<string, string>)[f.filter_type] ||
          f.filter_type
        const valueLabel = formatFilterValue(f.filter_type, f.value)
        return {
          id: f.id,
          title: f.name,
          subtitle: typeLabel,
          meta: valueLabel,
          badge: f.enabled
            ? { label: c.federationFilterEnabled, tone: 'success' }
            : { label: c.federationFilterDisabled, tone: 'muted' },
          busy: !!filterBusy[f.id],
          actions: [
            {
              key: 'toggle',
              label: f.enabled
                ? c.federationFilterDisableAction
                : c.federationFilterEnableAction,
              variant: f.enabled ? 'secondary' : 'primary',
              onClick: () => void toggleFilter(f),
              loading: filterBusy[f.id] === 'toggle',
            },
            {
              key: 'delete',
              label: t.common.delete,
              variant: 'danger',
              onClick: () => void deleteFilter(f.id),
              loading: filterBusy[f.id] === 'delete',
            },
          ],
        }
      }),
    [
      filteredContentFilters,
      filterTypeLabels,
      formatFilterValue,
      filterBusy,
      c.federationFilterEnabled,
      c.federationFilterDisabled,
      c.federationFilterDisableAction,
      c.federationFilterEnableAction,
      t.common.delete,
      toggleFilter,
      deleteFilter,
    ],
  )

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
      sectionId={sectionId}
    >
      <AutoHeight
        contentKey={`${identity?.actor_url ?? 'none'}-${instances.length}-${filters.length}`}
        className="federation-content-height"
      >
        <SettingGroup
          title={c.federationKeysIdentity}
          description={c.federationKeysIdentityDesc}
          {...bindGuide('federation.keys', g.federation.keys)}
          icon={<FaKey />}
        >
          <InfoActionCard
            empty={!identity}
            emptyText={c.federationKeysNoIdentity}
            copyLabel={t.common.copy}
            copiedLabel={t.common.copied}
            fields={
              identity
                ? ([
                    {
                      key: 'handle',
                      label: c.federationIdentityHandle,
                      value:
                        identity.handle ||
                        identity.acct ||
                        identity.username ||
                        '—',
                    },
                    {
                      key: 'actor',
                      label: c.federationIdentityActor,
                      value: identity.actor_url || '—',
                      mono: true,
                    },
                    ...(identity.key_id
                      ? [
                          {
                            key: 'key',
                            label: c.federationIdentityKeyId,
                            value: identity.key_id,
                            mono: true,
                          },
                        ]
                      : []),
                  ] satisfies InfoActionField[])
                : undefined
            }
            actions={[
              {
                key: 'rotate',
                label: c.federationKeysRotate,
                onClick: () => void rotateKeys(),
                disabled: rotatingKeys || !identity,
                loading: rotatingKeys,
                variant: 'secondary',
                confirm: c.federationKeysRotateConfirm,
                title: g.federation.rotateKeys.what,
              },
            ]}
            footer={
              <span
                className="has-guide-anchor"
                {...guideDomProps('federation.rotateKeys')}
              >
                <SettingTitleGuideEntry
                  title={c.federationKeysRotate}
                  guide={
                    bindGuide('federation.rotateKeys', g.federation.rotateKeys)
                      .guide
                  }
                />
                <span className="setting-description">
                  {g.federation.rotateKeys.notes}
                </span>
              </span>
            }
          />
        </SettingGroup>

        <SettingGroup
          title={c.federationInstancePolicy}
          description={c.federationInstancePolicyDesc}
          {...bindGuide('federation.policy', g.federation.policy)}
          icon={<LuShieldCheck />}
        >
          <SelectItem
            itemKey="fed-min-trust"
            label={c.federationMinTrustInbound}
            description={c.federationMinTrustInboundDesc}
            {...bindGuide('federation.minTrust', g.federation.minTrust)}
            value={String(policyDraft.minTrust)}
            onChange={(v) => onPolicyChange({ minTrust: Number(v) })}
            options={trustLevelOptions}
            layout="vertical"
          />

          <InputItem
            itemKey="fed-allowlist"
            label={c.federationAllowlistDomains}
            description={c.federationAllowlistDomainsDesc}
            {...bindGuide('federation.allowlist', g.federation.allowlist)}
            value={policyDraft.allowlistText}
            onChange={(allowlistText) => onPolicyChange({ allowlistText })}
            placeholder={c.federationAllowlistPlaceholder}
            multiline
            rows={4}
            layout="vertical"
          />

          <SwitchItem
            itemKey="fed-auto-discover"
            label={c.federationAutoDiscover}
            description={c.federationAutoDiscoverDesc}
            {...bindGuide('federation.autoDiscover', g.federation.autoDiscover)}
            value={policyDraft.autoDiscover}
            onChange={(autoDiscover) => onPolicyChange({ autoDiscover })}
          />
        </SettingGroup>

        <SettingGroup
          title={c.federationKnownInstances}
          description={c.federationKnownInstancesDesc}
          {...bindGuide('federation.knownInstances', g.federation.knownInstances)}
          icon={<FaServer />}
        >
          <ManagedList
            stats={instanceStats}
            queryToggleLabel={c.federationListQueryToggle}
            queryToggleDescription={c.federationListQueryToggleDesc}
            queryToggleIcon={<FaSearch aria-hidden />}
            queryCollapseLabel={c.federationListQueryCollapse}
            queryCollapseDescription={c.federationListQueryCollapseDesc}
            search={{
              value: instanceSearch,
              onChange: setInstanceSearch,
              placeholder: c.federationInstanceSearchPlaceholder,
              ariaLabel: c.federationInstanceSearchAria,
            }}
            filters={{
              options: instanceFilterOptions,
              value: instanceFilter,
              onChange: setInstanceFilter,
              ariaLabel: c.federationInstanceFilterAria,
            }}
            items={instanceListItems}
            emptyText={instanceEmptyText}
            footer={instanceFooter}
            maxHeight={instances.length > 8 ? '22rem' : null}
            maxVisibleItems={
              instances.length > 8 ? INSTANCE_LIST_CAP : null
            }
            truncateFooter={(shown, total) =>
              format(c.federationInstanceShowing, { shown, total })
            }
          />
        </SettingGroup>

        <SettingGroup
          title={c.federationContentFilters}
          description={c.federationContentFiltersDesc}
          {...bindGuide('federation.contentFilters', g.federation.contentFilters)}
          icon={<FaFilter />}
        >
          <ManagedList
            stats={filterStats}
            queryToggleLabel={c.federationListQueryToggle}
            queryToggleDescription={c.federationListQueryToggleDesc}
            queryToggleIcon={<FaSearch aria-hidden />}
            queryCollapseLabel={c.federationListQueryCollapse}
            queryCollapseDescription={c.federationListQueryCollapseDesc}
            search={{
              value: contentFilterSearch,
              onChange: setContentFilterSearch,
              placeholder: c.federationFilterSearchPlaceholder,
              ariaLabel: c.federationFilterSearchAria,
            }}
            filters={{
              options: contentFilterChipOptions,
              value: contentFilterChip,
              onChange: setContentFilterChip,
              ariaLabel: c.federationFilterFilterAria,
            }}
            items={filterListItems}
            emptyText={contentFilterEmptyText}
            footer={contentFilterFooter}
            maxHeight={null}
            formTitle={c.federationAddFilter}
            formDescription={c.federationAddFilterDesc}
            formIcon={<FaPlus aria-hidden />}
            formCollapseLabel={t.common.cancel}
            formCollapseDescription={c.federationListQueryCollapseDesc}
            formOpen={filterFormOpen}
            onFormOpenChange={setFilterFormOpen}
            form={
              <>
                <InputItem
                  itemKey="fed-filter-name"
                  label={c.federationFilterName}
                  value={newFilterName}
                  onChange={setNewFilterName}
                  placeholder={c.federationFilterNamePlaceholder}
                  layout="vertical"
                />

                <SelectItem
                  itemKey="fed-filter-type"
                  label={c.federationFilterType}
                  description={filterTypeHelp[newFilterType]}
                  value={newFilterType}
                  onChange={handleFilterTypeChange}
                  options={filterTypeOptions}
                  layout="vertical"
                />

                {newFilterType === 'block_activity_type' && (
                  <SelectItem
                    itemKey="fed-filter-activity"
                    label={c.federationFilterActivityType}
                    hint={c.federationFilterDescBlockActivity}
                    value={
                      ACTIVITY_TYPES.includes(
                        newFilterValue as (typeof ACTIVITY_TYPES)[number],
                      )
                        ? newFilterValue
                        : 'Announce'
                    }
                    onChange={setNewFilterValue}
                    options={activityTypeOptions}
                    layout="vertical"
                  />
                )}

                {newFilterType === 'require_trust_level' && (
                  <SelectItem
                    itemKey="fed-filter-trust"
                    label={c.federationFilterTrustLevel}
                    hint={c.federationFilterDescRequireTrust}
                    value={
                      ['0', '1', '2', '3', '4'].includes(newFilterValue)
                        ? newFilterValue
                        : '0'
                    }
                    onChange={setNewFilterValue}
                    options={trustLevelOptions}
                    layout="vertical"
                  />
                )}

                {newFilterType === 'block_keyword' && (
                  <InputItem
                    itemKey="fed-filter-value"
                    label={c.federationFilterValue}
                    hint={c.federationFilterDescBlockKeyword}
                    value={newFilterValue}
                    onChange={setNewFilterValue}
                    placeholder={c.federationFilterValuePlaceholderKeyword}
                    layout="vertical"
                  />
                )}

                <div className="managed-list-form-actions">
                  <SettingsButton
                    variant="primary"
                    size="sm"
                    onClick={() => void addFilter()}
                  >
                    {c.federationAddFilter}
                  </SettingsButton>
                </div>
              </>
            }
          />
        </SettingGroup>

        <SettingGroup
          title={c.federationDeliveryQueue}
          description={c.federationDeliveryQueueDesc}
          {...bindGuide('federation.deliveryQueue', g.federation.deliveryQueue)}
          icon={<FaPaperPlane />}
        >
          <FederationDeliveryQueue
            stats={deliveryStats}
            items={deliveryItems}
            onStatsChange={setDeliveryStats}
            onItemsChange={setDeliveryItems}
            onMessage={onMessage}
            onRefresh={loadDelivery}
          />
        </SettingGroup>

        <SettingGroup
          title={c.federationAdvanced}
          description={c.federationAdvancedDesc}
          {...bindGuide('federation.advanced', g.federation.advanced)}
          icon={<FaCog />}
          collapsible
          defaultExpanded={false}
        >
          <NumberItem
            itemKey="fed-rate-max"
            label={c.federationRateMaxRequests}
            description={c.federationRateMaxRequestsDesc}
            {...bindGuide('federation.rateMax', g.federation.rateMax)}
            value={policyDraft.rateMax}
            onChange={(v) =>
              onPolicyChange({
                rateMax: Math.max(1, Math.min(1_000_000, v || 1)),
              })
            }
            min={1}
            max={1_000_000}
            step={1}
            layout="vertical"
          />
          <NumberItem
            itemKey="fed-rate-window"
            label={c.federationRateWindowSeconds}
            description={c.federationRateWindowSecondsDesc}
            {...bindGuide('federation.rateWindow', g.federation.rateWindow)}
            value={policyDraft.rateWindow}
            onChange={(v) =>
              onPolicyChange({
                rateWindow: Math.max(1, Math.min(86_400, v || 1)),
              })
            }
            min={1}
            max={86_400}
            step={1}
            unit="s"
            layout="vertical"
          />
          <NumberItem
            itemKey="fed-rate-trusted-mul"
            label={c.federationRateTrustedMultiplier}
            description={c.federationRateTrustedMultiplierDesc}
            {...bindGuide('federation.rateTrusted', g.federation.rateTrusted)}
            value={policyDraft.rateTrustedMul}
            onChange={(v) =>
              onPolicyChange({
                rateTrustedMul: Math.max(1, Math.min(100, v || 1)),
              })
            }
            min={1}
            max={100}
            step={1}
            layout="vertical"
          />
        </SettingGroup>
      </AutoHeight>
    </SettingSection>
  )
}

export default FederationConfigSection
