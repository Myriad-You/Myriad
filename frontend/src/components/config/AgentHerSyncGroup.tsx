import type { HerSync } from '../../services/agent/agentApi'
import { LuRefreshCw } from '@lib/icons'

import { useCallback, useEffect, useState } from 'react'
import { useConfigI18n as useI18n } from '../../contexts/I18nContext'
import { usePersonaPublicName } from '../../features/merope/persona/usePersonaPublicName'
import { agentService } from '../../services/agent'
import { showStickyToast } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
import { InfoActionCard, SettingGroup, SwitchItem } from '../settings'
import './AgentHerSyncGroup.css'

/** The consent text, section by section, in the order the owner settled (D4). */
const CONSENT_SECTIONS = [
  'Synced',
  'NotSynced',
  'OnComputer',
  'SiteReads',
  'Connection',
  'Backup',
  'OnMac',
  'TurnOff',
  'AfterOff',
  'Delete',
  'Region',
] as const

/** Run in the site folder before turning sync on; not translated. */
const BACKUP_COMMAND = 'BACKUP_MODE=local scripts/extra/backup.sh backup'

/**
 * Whether the site owner syncs her across their computers. Only the site owner
 * sees it; off by default. Turning it on first shows the consent text, and only
 * its own button turns sync on; turning it off needs no reading.
 */
export function AgentHerSyncGroup() {
  const { t, format } = useI18n()
  const c = t.config
  const name = usePersonaPublicName()
  const [sync, setSync] = useState<HerSync | null>(null)
  const [working, setWorking] = useState(false)
  const [reading, setReading] = useState(false)

  const fail = useCallback((error: unknown, fallback: string) => {
    showStickyToast({
      message: userFacingError(error, fallback),
      type: 'error',
      replaceKey: 'config-her-sync',
    })
  }, [])

  useEffect(() => {
    let cancelled = false
    agentService
      .getHerSync()
      .then((shown) => {
        if (!cancelled) setSync(shown)
      })
      .catch((error: unknown) => fail(error, c.agentHerSyncLoadFailed))
    return () => {
      cancelled = true
    }
  }, [c.agentHerSyncLoadFailed, fail])

  const turn = useCallback(
    async (on: boolean) => {
      setWorking(true)
      try {
        setSync(await agentService.setHerSync(on))
        setReading(false)
      } catch (error) {
        fail(error, c.agentHerSyncSaveFailed)
      } finally {
        setWorking(false)
      }
    },
    [c.agentHerSyncSaveFailed, fail],
  )

  const words = { name, days: String(sync?.retentionDays ?? 30) }

  return (
    <SettingGroup
      title={c.agentHerSyncTitle}
      icon={<LuRefreshCw />}
      description={format(c.agentHerSyncDesc, words)}
    >
      <SwitchItem
        itemKey="agent-her-sync"
        label={c.agentHerSyncLabel}
        description={
          sync && !sync.available
            ? c.agentHerSyncUnavailable
            : sync && !sync.on && sync.stoppedAt
              ? format(c.agentHerSyncStopped, {
                  days: String(sync.retentionDays),
                })
              : undefined
        }
        value={sync?.on ?? false}
        onChange={(on) => {
          if (on) setReading(true)
          else void turn(false)
        }}
        disabled={!sync?.available || reading}
        loading={sync === null || working}
      />
      {reading && sync?.available && !sync.on ? (
        <InfoActionCard
          title={c.agentHerSyncConsentTitle}
          tone="info"
          className="her-sync-consent"
          actions={[
            {
              key: 'open',
              label: c.agentHerSyncConsentOpen,
              onClick: () => void turn(true),
              disabled: working,
              loading: working,
              variant: 'primary',
            },
            {
              key: 'cancel',
              label: t.common.cancel,
              onClick: () => setReading(false),
              disabled: working,
              variant: 'secondary',
            },
          ]}
        >
          {CONSENT_SECTIONS.map((section) => (
            <section key={section} className="her-sync-consent-section">
              <h4>
                {format(c[`agentHerSyncConsent${section}Head`], words)}
              </h4>
              <ul>
                {format(c[`agentHerSyncConsent${section}Body`], words)
                  .split('\n')
                  .map((line) => (
                    <li key={line}>{line}</li>
                  ))}
              </ul>
            </section>
          ))}
          {/* Above the buttons: back up once, then agreeing is turning it on. */}
          <p className="her-sync-consent-note">
            {c.agentHerSyncConsentBackup}
          </p>
          <code className="her-sync-consent-command">{BACKUP_COMMAND}</code>
          <p className="her-sync-consent-note">{c.agentHerSyncConsentAgree}</p>
        </InfoActionCard>
      ) : null}
    </SettingGroup>
  )
}

export default AgentHerSyncGroup
