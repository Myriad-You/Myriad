import type { SnapshotsResponse, UpdaterStatus } from '../../../services/updaterApi'
import type { U } from './helpers'
import React, { useMemo } from 'react'
import {
  clampSnapshotLimit,
  SNAPSHOT_LIMIT_DEFAULT,
  SNAPSHOT_LIMIT_PRESETS,
} from '../../../services/updaterApi'
import {
  FieldSelect,
  SettingTitleGuideEntry,
  ToggleSwitch,
  useSettingGuide,
} from '../../settings'
import { format } from './helpers'

export interface SnapshotLimitPrefsProps {
  status: UpdaterStatus | null
  snapshotStats?: Pick<
    SnapshotsResponse,
    'eligible_count' | 'protected_count' | 'total_count' | 'snapshot_limit'
  > | null
  disabled: boolean
  saving?: boolean
  u: U
  onSave: (prefs: {
    snapshot_limit_enabled?: boolean
    snapshot_limit?: number
  }) => void | Promise<void>
}

export function statusHasSnapshotLimitFields(
  status: UpdaterStatus | null | undefined,
): boolean {
  if (!status) return false
  return (
    typeof status.snapshot_limit_enabled === 'boolean' &&
    typeof status.snapshot_limit === 'number'
  )
}

export const SnapshotLimitPrefs: React.FC<SnapshotLimitPrefsProps> = ({
  status,
  snapshotStats = null,
  disabled,
  saving = false,
  u,
  onSave,
}) => {
  const { catalog: g, bindGuide } = useSettingGuide()
  const limitGuide = bindGuide(
    'updater.snapshotLimit',
    g.updater.snapshotLimit,
  ).guide

  const limitFieldsKnown = statusHasSnapshotLimitFields(status)
  // ON only if fields present-and-true, or known-absent (old backend)
  const limitEnabled = status?.snapshot_limit_enabled !== false
  const rawLimit = status?.snapshot_limit ?? SNAPSHOT_LIMIT_DEFAULT
  const limitValue = clampSnapshotLimit(rawLimit)
  const knownLimit = (SNAPSHOT_LIMIT_PRESETS as readonly number[]).includes(
    limitValue as (typeof SNAPSHOT_LIMIT_PRESETS)[number],
  )

  const limitOptions = useMemo(() => {
    const base = SNAPSHOT_LIMIT_PRESETS.map((n) => ({
      value: String(n),
      label: format(u.updaterSnapshotLimitOption, { n: String(n) }),
    }))
    if (!knownLimit) {
      base.push({
        value: String(limitValue),
        label: format(u.updaterSnapshotLimitOption, {
          n: String(limitValue),
        }),
      })
      return base.toSorted((a, b) => Number(a.value) - Number(b.value))
    }
    return base
  }, [knownLimit, limitValue, u.updaterSnapshotLimitOption])

  const inactive = disabled || !status || saving
  const desc = limitEnabled
    ? format(u.updaterSnapshotLimitEnabledDescOn, {
        n: String(limitValue),
      })
    : u.updaterSnapshotLimitEnabledDescOff

  const eligible =
    typeof snapshotStats?.eligible_count === 'number'
      ? snapshotStats.eligible_count
      : null
  const protectedCount =
    typeof snapshotStats?.protected_count === 'number'
      ? snapshotStats.protected_count
      : null
  const countsLine =
    limitEnabled && eligible != null && protectedCount != null
      ? format(u.updaterSnapshotLimitCounts, {
          eligible: String(eligible),
          n: String(limitValue),
          protected: String(protectedCount),
        })
      : null

  return (
    <div
      id="cfg-g-updater-snapshotLimit"
      data-guide-path="updater.snapshotLimit"
      className="updater-snapshot-limit has-guide-anchor"
    >
      <div className="updater-snapshot-limit-text">
        <span className="updater-snapshot-limit-title">
          {u.updaterSnapshotLimitEnabled}
          <SettingTitleGuideEntry
            title={u.updaterSnapshotLimitEnabled}
            guide={limitGuide}
          />
        </span>
        <span className="updater-snapshot-limit-desc">{desc}</span>
        {countsLine && (
          <span className="updater-snapshot-limit-counts">{countsLine}</span>
        )}
        {status && !limitFieldsKnown && (
          <span className="updater-snapshot-limit-warn" role="status">
            {u.updaterSnapshotLimitUpdaterOld}
          </span>
        )}
      </div>
      <div className="updater-snapshot-limit-controls">
        {limitEnabled && (
          <FieldSelect
            value={String(limitValue)}
            options={limitOptions}
            disabled={inactive}
            aria-label={u.updaterSnapshotLimitCount}
            size="sm"
            onChange={(v) => {
              void onSave({ snapshot_limit: clampSnapshotLimit(Number(v)) })
            }}
          />
        )}
        <ToggleSwitch
          checked={limitEnabled}
          disabled={inactive}
          aria-label={u.updaterSnapshotLimitEnabled}
          onChange={(checked) => {
            void onSave({ snapshot_limit_enabled: checked })
          }}
        />
      </div>
    </div>
  )
}

export default SnapshotLimitPrefs
