import type { HerSync } from '../../services/agent/agentApi'
import { LuRefreshCw } from '@lib/icons'

import { useCallback, useEffect, useState } from 'react'
import { useConfigI18n as useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'
import { showStickyToast } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
import { InfoCard, SettingGroup, SwitchItem } from '../settings'

/** Whether the site owner syncs her across their computers. Only the site owner sees it; off by default. */
export function AgentHerSyncGroup() {
  const { t, format } = useI18n()
  const c = t.config
  const [sync, setSync] = useState<HerSync | null>(null)
  const [working, setWorking] = useState(false)

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
      } catch (error) {
        fail(error, c.agentHerSyncSaveFailed)
      } finally {
        setWorking(false)
      }
    },
    [c.agentHerSyncSaveFailed, fail],
  )

  return (
    <SettingGroup
      title={c.agentHerSyncTitle}
      icon={<LuRefreshCw />}
      description={c.agentHerSyncDesc}
    >
      {/* The consent text goes here once the owner has settled it (D4). */}
      <InfoCard content={c.agentHerSyncConsentPending} />
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
        onChange={(on) => void turn(on)}
        disabled={!sync?.available}
        loading={sync === null || working}
      />
    </SettingGroup>
  )
}

export default AgentHerSyncGroup
