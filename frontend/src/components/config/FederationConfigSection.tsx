/**
 * 联邦信任策略管理（管理员）
 * - allowlist / min_trust / auto_discover
 * - 实例列表：信任层级 + 封禁
 * - 内容过滤规则 CRUD
 */

import type {
  ContentFilterItem,
  FederationInstance,
} from '../../types/federation'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { federationApi } from '../../services/federationApi'
import {
  ButtonItem,
  InputItem,
  NumberItem,
  SelectItem,
  SettingGroup,
  SettingSection,
  SwitchItem,
} from '../settings'

interface FederationConfigSectionProps {
  title: string
  icon: React.ReactNode
  description: string
  sectionId?: string
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

/**
 * Known ActivityPub + MFP activity `type` values handled (or accepted) by the
 * federation inbox. Values are stored as-is for `block_activity_type` filters.
 * Align with `backend/src/federation/inbox.rs`.
 */
const ACTIVITY_TYPES = [
  // Standard ActivityPub
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
  // MFP extensions (inbox whitelist)
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

/**
 * Fallback when a stored activity type has no i18n entry (custom / future types).
 * Prefer `activityTypeLabels` from config keys when available.
 */
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
> = ({ title, icon, description, sectionId, onMessage }) => {
  const { t } = useI18n()
  const c = t.config
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [instances, setInstances] = useState<FederationInstance[]>([])
  const [filters, setFilters] = useState<ContentFilterItem[]>([])

  // Policy draft
  const [minTrust, setMinTrust] = useState(0)
  const [allowlistText, setAllowlistText] = useState('')
  const [autoDiscover, setAutoDiscover] = useState(true)
  // Advanced rate limit (defaults match backend RateLimitPolicy)
  const [rateMax, setRateMax] = useState(100)
  const [rateWindow, setRateWindow] = useState(60)
  const [rateTrustedMul, setRateTrustedMul] = useState(5)

  // New filter draft
  const [newFilterName, setNewFilterName] = useState('')
  const [newFilterType, setNewFilterType] = useState<FilterType>('block_keyword')
  const [newFilterValue, setNewFilterValue] = useState('')

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

  /** Maps stored ActivityPub type values → localized display labels. */
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

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const [p, inst, f] = await Promise.all([
        federationApi.getTrustPolicy(),
        federationApi.getInstances().catch(() => ({ instances: [], total: 0 })),
        federationApi
          .listContentFilters()
          .catch(() => ({ filters: [], total: 0 })),
      ])
      setMinTrust(p.min_trust_level ?? 0)
      setAllowlistText((p.allowed_domains || []).join('\n'))
      setAutoDiscover(p.auto_discover !== false)
      setRateMax(p.rate_limit?.max_requests_per_window ?? 100)
      setRateWindow(p.rate_limit?.window_seconds ?? 60)
      setRateTrustedMul(p.rate_limit?.trusted_multiplier ?? 5)
      setInstances(inst.instances || [])
      setFilters(f.filters || [])
    } catch (e) {
      onMessage?.(
        e instanceof Error ? e.message : c.federationLoadFailed,
        'error',
      )
    } finally {
      setLoading(false)
    }
  }, [onMessage, c.federationLoadFailed])

  useEffect(() => {
    void load()
  }, [load])

  const savePolicy = async () => {
    setSaving(true)
    try {
      const domains = allowlistText
        .split(/[\n,]+/)
        .map((s) => s.trim().toLowerCase())
        .filter(Boolean)
      await federationApi.updateTrustPolicy({
        min_trust_level: minTrust,
        allowed_domains: domains,
        auto_discover: autoDiscover,
        rate_limit: {
          max_requests_per_window: rateMax,
          window_seconds: rateWindow,
          trusted_multiplier: rateTrustedMul,
        },
      })
      onMessage?.(c.federationPolicySaved, 'success')
      await load()
    } catch (e) {
      onMessage?.(
        e instanceof Error ? e.message : c.federationSaveFailed,
        'error',
      )
    } finally {
      setSaving(false)
    }
  }

  const resetRateDefaults = () => {
    setRateMax(100)
    setRateWindow(60)
    setRateTrustedMul(5)
  }

  const setInstanceTrust = async (domain: string, level: number) => {
    try {
      await federationApi.updateInstanceTrust({ domain, trust_level: level })
      setInstances((prev) =>
        prev.map((i) =>
          i.domain === domain ? { ...i, trust_level: level } : i,
        ),
      )
    } catch (e) {
      onMessage?.(
        e instanceof Error ? e.message : c.federationUpdateFailed,
        'error',
      )
    }
  }

  const toggleBlock = async (domain: string, block: boolean) => {
    try {
      await federationApi.toggleInstanceBlock({ domain, block })
      setInstances((prev) =>
        prev.map((i) =>
          i.domain === domain ? { ...i, blocked: block } : i,
        ),
      )
    } catch (e) {
      onMessage?.(
        e instanceof Error ? e.message : c.federationBlockFailed,
        'error',
      )
    }
  }

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

  const formatFilterSummary = useCallback(
    (f: ContentFilterItem): string => {
      const typeLabel =
        (filterTypeLabels as Record<string, string>)[f.filter_type] ||
        f.filter_type
      const valueLabel = formatFilterValue(f.filter_type, f.value)
      return `${typeLabel} · ${valueLabel}`
    },
    [filterTypeLabels, formatFilterValue],
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
      await load()
      onMessage?.(c.federationFilterAdded, 'success')
    } catch (e) {
      onMessage?.(
        e instanceof Error ? e.message : c.federationAddFilterFailed,
        'error',
      )
    }
  }

  const toggleFilter = async (f: ContentFilterItem) => {
    try {
      await federationApi.updateContentFilter(f.id, { enabled: !f.enabled })
      setFilters((prev) =>
        prev.map((x) =>
          x.id === f.id ? { ...x, enabled: !x.enabled } : x,
        ),
      )
    } catch (e) {
      onMessage?.(
        e instanceof Error ? e.message : c.federationUpdateFailed,
        'error',
      )
    }
  }

  const deleteFilter = async (id: number) => {
    try {
      await federationApi.deleteContentFilter(id)
      setFilters((prev) => prev.filter((x) => x.id !== id))
    } catch (e) {
      onMessage?.(
        e instanceof Error ? e.message : c.federationUpdateFailed,
        'error',
      )
    }
  }

  if (loading) {
    return (
      <SettingSection
        title={title}
        icon={icon}
        description={description}
        sectionId={sectionId}
      >
        <p className="text-sm text-gray-500">{t.common?.loading || '…'}</p>
      </SettingSection>
    )
  }

  const saveButtonText =
    saving ? '…' : c.federationSavePolicy || t.common?.save || 'Save'

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
      sectionId={sectionId}
    >
      <SettingGroup
        title={c.federationInstancePolicy}
        description={c.federationTrustLevelHelp}
      >
        <SelectItem
          itemKey="fed-min-trust"
          label={c.federationMinTrustInbound}
          description={c.federationMinTrustInboundDesc}
          value={String(minTrust)}
          onChange={(v) => setMinTrust(Number(v))}
          options={trustLevelOptions}
          layout="vertical"
        />

        <InputItem
          itemKey="fed-allowlist"
          label={c.federationAllowlistDomains}
          description={c.federationAllowlistDomainsDesc}
          value={allowlistText}
          onChange={setAllowlistText}
          placeholder={c.federationAllowlistPlaceholder}
          multiline
          rows={4}
          layout="vertical"
        />

        <SwitchItem
          itemKey="fed-auto-discover"
          label={c.federationAutoDiscover}
          description={c.federationAutoDiscoverDesc}
          value={autoDiscover}
          onChange={setAutoDiscover}
        />

        <ButtonItem
          label={c.federationInstancePolicy}
          buttonText={saveButtonText}
          onClick={() => void savePolicy()}
          disabled={saving}
          loading={saving}
        />
      </SettingGroup>

      <SettingGroup
        title={c.federationKnownInstances}
        description={c.federationKnownInstancesDesc}
      >
        {instances.length === 0 ? (
          <p className="text-sm text-gray-500">{c.federationNoInstances}</p>
        ) : (
          <ul className="space-y-2">
            {instances.map((inst) => (
              <li
                key={inst.domain}
                className="flex flex-wrap items-center gap-2 rounded-lg border border-black/5 bg-black/[0.02] px-3 py-2.5 text-sm dark:border-white/5 dark:bg-white/[0.03]"
              >
                <span className="min-w-0 flex-1 font-medium truncate">
                  {inst.domain}
                  {inst.blocked && (
                    <span className="ml-2 text-xs text-red-500">
                      {c.federationBlocked}
                    </span>
                  )}
                </span>
                <select
                  className="rounded-lg border border-black/10 bg-white/50 px-2 py-1.5 text-xs dark:border-white/10 dark:bg-black/20"
                  value={inst.trust_level}
                  onChange={(e) =>
                    void setInstanceTrust(inst.domain, Number(e.target.value))
                  }
                  aria-label={c.federationMinTrustInbound}
                >
                  {trustLevels.map((l) => (
                    <option key={l.value} value={l.value}>
                      {l.label}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  className={
                    `rounded-full px-2.5 py-1 text-xs font-medium ${
                    inst.blocked
                      ? 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300'
                      : 'bg-red-500/10 text-red-600 dark:text-red-300'}`
                  }
                  onClick={() => void toggleBlock(inst.domain, !inst.blocked)}
                >
                  {inst.blocked ? c.federationUnblock : c.federationBlock}
                </button>
              </li>
            ))}
          </ul>
        )}
      </SettingGroup>

      <SettingGroup
        title={c.federationContentFilters}
        description={c.federationContentFiltersDesc}
      >
        <div className="space-y-1 mb-3">
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

          <ButtonItem
            label={c.federationContentFilters}
            buttonText={c.federationAddFilter}
            onClick={() => void addFilter()}
          />
        </div>

        {filters.length === 0 ? (
          <p className="text-sm text-gray-500">{c.federationNoFilters}</p>
        ) : (
          <ul className="space-y-1.5">
            {filters.map((f) => (
              <li
                key={f.id}
                className="flex flex-wrap items-center gap-2 rounded-lg border border-black/5 bg-black/[0.02] px-3 py-2.5 text-sm dark:border-white/5 dark:bg-white/[0.03]"
              >
                <span className="min-w-0 flex-1 truncate">
                  <strong>{f.name}</strong>{' '}
                  <span className="text-xs text-gray-500 dark:text-gray-400">
                    {formatFilterSummary(f)}
                  </span>
                </span>
                <button
                  type="button"
                  className="text-xs rounded-full px-2.5 py-1 font-medium bg-black/5 dark:bg-white/10"
                  onClick={() => void toggleFilter(f)}
                >
                  {f.enabled
                    ? c.federationFilterEnabled
                    : c.federationFilterDisabled}
                </button>
                <button
                  type="button"
                  className="text-xs font-medium text-red-500"
                  onClick={() => void deleteFilter(f.id)}
                >
                  {t.common?.delete || 'Delete'}
                </button>
              </li>
            ))}
          </ul>
        )}
      </SettingGroup>

      <SettingGroup
        title={c.federationAdvanced}
        description={c.federationAdvancedDesc}
        collapsible
        defaultExpanded={false}
      >
        <NumberItem
          itemKey="fed-rate-max"
          label={c.federationRateMaxRequests}
          description={c.federationRateMaxRequestsDesc}
          value={rateMax}
          onChange={(v) => setRateMax(Math.max(1, Math.min(1_000_000, v || 1)))}
          min={1}
          max={1_000_000}
          step={1}
          layout="vertical"
        />
        <NumberItem
          itemKey="fed-rate-window"
          label={c.federationRateWindowSeconds}
          description={c.federationRateWindowSecondsDesc}
          value={rateWindow}
          onChange={(v) => setRateWindow(Math.max(1, Math.min(86_400, v || 1)))}
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
          value={rateTrustedMul}
          onChange={(v) =>
            setRateTrustedMul(Math.max(1, Math.min(100, v || 1)))
          }
          min={1}
          max={100}
          step={1}
          layout="vertical"
        />
        <ButtonItem
          label={c.federationRateLimit}
          description={c.federationRateLimitDesc}
          buttonText={c.federationRateResetDefaults}
          onClick={resetRateDefaults}
          variant="secondary"
        />
        <ButtonItem
          label={c.federationAdvanced}
          buttonText={saveButtonText}
          onClick={() => void savePolicy()}
          disabled={saving}
          loading={saving}
        />
      </SettingGroup>
    </SettingSection>
  )
}

export default FederationConfigSection
