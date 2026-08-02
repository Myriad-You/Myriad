/**
 * Snapshot retention prefs above the backup list.
 * Plain label + controls — no card chrome / hover effects.
 */

import type { UpdaterStatus } from '../../../services/updaterApi'
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
  disabled: boolean
  saving?: boolean
  u: U
  onSave: (prefs: {
    snapshot_limit_enabled?: boolean
    snapshot_limit?: number
  }) => void | Promise<void>
}

export const SnapshotLimitPrefs: React.FC<SnapshotLimitPrefsProps> = ({
  status,
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

  // Default ON when field omitted (older updaters / historical prune(3)).
  const limitEnabled = status?.snapshot_limit_enabled !== false
  const rawLimit = status?.snapshot_limit ?? SNAPSHOT_LIMIT_DEFAULT
  // Show any in-range value (1–20), not only presets — BE may store 4, 7, …
  const limitValue = clampSnapshotLimit(rawLimit)
  const knownLimit = (SNAPSHOT_LIMIT_PRESETS as readonly number[]).includes(
    limitValue as (typeof SNAPSHOT_LIMIT_PRESETS)[number],
  )

  const limitOptions = useMemo(() => {
    const base = SNAPSHOT_LIMIT_PRESETS.map((n) => ({
      value: String(n),
      label: format(u.updaterSnapshotLimitOption, { n: String(n) }),
    }))
    // Surface non-preset current values so the select stays controlled.
    if (!knownLimit) {
      base.push({
        value: String(limitValue),
        label: format(u.updaterSnapshotLimitOption, {
          n: String(limitValue),
        }),
      })
      base.sort((a, b) => Number(a.value) - Number(b.value))
    }
    return base
  }, [knownLimit, limitValue, u.updaterSnapshotLimitOption])

  const inactive = disabled || !status || saving
  const desc = limitEnabled
    ? format(u.updaterSnapshotLimitEnabledDescOn, {
        n: String(limitValue),
      })
    : u.updaterSnapshotLimitEnabledDescOff

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
