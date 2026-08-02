/**
 * Updater advanced / transport / digests panel.
 */

import type { ReleaseManifest, TransportMode, UpdaterStatus } from '../../../services/updaterApi'
import type { U } from './helpers'
import { LuRefreshCw } from '@lib/icons'
import React from 'react'
import {
  InputItem,
  SegmentedControl,
  SettingsButton,
  SettingTitleGuideEntry,
  useSettingGuide,
} from '../../settings'

export function AdvancedPanel({
  status,
  available,
  transport,
  token,
  loading,
  u,
  onTransportChange,
  onTokenChange,
  onRefresh,
}: {
  status: UpdaterStatus | null
  available: ReleaseManifest | null
  transport: TransportMode
  token: string
  loading: boolean
  u: U
  onTransportChange: (m: TransportMode) => void
  onTokenChange: (s: string) => void
  onRefresh: () => void
}) {
  const { catalog: g, bindGuide } = useSettingGuide()
  const transportBinding = bindGuide('updater.transport', g.updater.transport)
  const tokenBinding = bindGuide('updater.token', g.updater.token)
  const transportGuide = transportBinding.guide
  const tokenGuide = tokenBinding.guide

  return (
    <>
      <dl className="updater-detail-grid">
        <dt>{u.updaterUpdaterVersion}</dt>
        <dd>{status?.updater_version ?? '—'}</dd>
        <dt>{u.updaterChannelLabel}</dt>
        <dd>
          {status
            ? `${status.channel} (${status.update_mode ?? 'release'})`
            : '—'}
        </dd>
        <dt>{u.updaterJobInFlight}</dt>
        <dd>{status?.job_in_flight ?? u.updaterNone}</dd>
      </dl>

      {available && (
        <details className="updater-digests">
          <summary>{u.updaterImageDigests}</summary>
          <dl className="updater-detail-grid">
            {Object.entries(available.images).map(([k, v]) => (
              <React.Fragment key={k}>
                <dt>{k}</dt>
                <dd>{v.digest}</dd>
              </React.Fragment>
            ))}
          </dl>
        </details>
      )}

      <div
        id="cfg-g-updater-transport"
        data-guide-path="updater.transport"
        className="updater-transport has-guide-anchor"
      >
        <div className="updater-transport-label" id="updater-transport-label">
          {u.updaterTransport}
          <SettingTitleGuideEntry
            title={u.updaterTransport}
            guide={transportGuide}
          />
        </div>
        <SegmentedControl
          size="sm"
          columns={2}
          value={transport}
          options={[
            { value: 'backend' as const, label: u.updaterTransportBackend },
            { value: 'direct' as const, label: u.updaterTransportDirect },
          ]}
          onChange={onTransportChange}
          ariaLabel={u.updaterTransport}
        />
        {transport === 'direct' && (
          <>
            <p className="updater-transport-hint">
              {u.updaterTransportDirectHint}
            </p>
            <InputItem
              itemKey="updater-transport-token"
              label="UPDATE_TOKEN"
              value={token}
              onChange={onTokenChange}
              placeholder="UPDATE_TOKEN"
              inputType="password"
              autoComplete="off"
              layout="vertical"
              size="sm"
              className="updater-token-input-item"
              guide={tokenGuide}
              guidePath="updater.token"
            />
          </>
        )}
      </div>

      <div className="updater-advanced-actions">
        <SettingsButton
          variant="secondary"
          onClick={onRefresh}
          disabled={loading}
          loading={loading}
          icon={<LuRefreshCw size={13} />}
        >
          {u.updaterRefresh}
        </SettingsButton>
      </div>
    </>
  )
}
