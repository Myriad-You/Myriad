import type { SyncDevice, SyncDeviceList } from '../../services/agent/agentApi'
import type { ManagedListItem, ManagedListStat } from '../settings/ManagedList'
import { LuMonitor, LuPlus } from '@lib/icons'

import { useCallback, useEffect, useMemo, useState } from 'react'
import { useConfigI18n as useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'
import { ApiError } from '../../services/api'
import { showStickyToast } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
import {
  InfoActionCard,
  InputItem,
  ManagedList,
  SettingGroup,
  SettingsButton,
} from '../settings'

/** Devices that sync her with this site. Only the site owner sees it. */
export function AgentDevicesGroup() {
  const { t, format, locale } = useI18n()
  const c = t.config
  const [list, setList] = useState<SyncDeviceList | null>(null)
  const [loading, setLoading] = useState(false)
  const [working, setWorking] = useState(false)
  const [formOpen, setFormOpen] = useState(false)
  const [name, setName] = useState('')
  const [issued, setIssued] = useState<{ name: string; token: string } | null>(
    null,
  )

  const fail = useCallback((error: unknown, fallback: string) => {
    showStickyToast({
      message: userFacingError(error, fallback),
      type: 'error',
      replaceKey: 'config-sync-devices',
    })
  }, [])

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setList(await agentService.listSyncDevices())
    } catch (error) {
      fail(error, c.agentDevicesLoadFailed)
    } finally {
      setLoading(false)
    }
  }, [c.agentDevicesLoadFailed, fail])

  useEffect(() => {
    void load()
  }, [load])

  const add = useCallback(async () => {
    const trimmed = name.trim()
    if (!trimmed) return
    setWorking(true)
    try {
      const { device, token } = await agentService.addSyncDevice(trimmed)
      setIssued({ name: device.name, token })
      setName('')
      setFormOpen(false)
      await load()
    } catch (error) {
      if (error instanceof ApiError && error.code === 'device_limit') {
        fail(error, format(c.agentDevicesLimit, { n: list?.limit ?? 10 }))
      } else {
        fail(error, c.agentDevicesAddFailed)
      }
    } finally {
      setWorking(false)
    }
  }, [c, fail, format, list?.limit, load, name])

  const remove = useCallback(
    async (device: SyncDevice) => {
      setWorking(true)
      try {
        await agentService.removeSyncDevice(device.id)
        await load()
      } catch (error) {
        fail(error, c.agentDevicesRemoveFailed)
      } finally {
        setWorking(false)
      }
    },
    [c.agentDevicesRemoveFailed, fail, load],
  )

  const setRole = useCallback(
    async (device: SyncDevice, role: SyncDevice['role']) => {
      setWorking(true)
      try {
        setList(await agentService.setSyncDeviceRole(device.id, role))
      } catch (error) {
        fail(error, c.agentDevicesRoleFailed)
      } finally {
        setWorking(false)
      }
    },
    [c.agentDevicesRoleFailed, fail],
  )

  const when = useCallback(
    (at: string) => new Date(at).toLocaleString(locale || undefined),
    [locale],
  )

  const devices = useMemo(() => list?.devices ?? [], [list])
  const live = devices.filter((device) => !device.revokedAt)
  const parentName =
    list && list.parent !== 'site'
      ? (devices.find((device) => device.id === list.parent)?.name ??
        c.agentDevicesParentSite)
      : c.agentDevicesParentSite

  const stats: ManagedListStat[] = [
    {
      key: 'live',
      label: c.agentDevicesStatLive,
      value: `${live.length} / ${list?.limit ?? 10}`,
      tone: live.length === 0 ? 'muted' : 'default',
    },
    {
      key: 'parent',
      label: c.agentDevicesStatParent,
      value: parentName,
      tone: 'default',
    },
  ]

  const removedWhy = (device: SyncDevice) =>
    device.revokedReason === 'device_left'
      ? c.agentDevicesRemovedLeft
      : device.revokedReason === 'owner_changed'
        ? c.agentDevicesRemovedOwnerChanged
        : c.agentDevicesRemoved

  const items: ManagedListItem[] = devices.map((device) => {
    const badges: ManagedListItem['badges'] = []
    if (device.revokedAt) {
      badges.push({ label: removedWhy(device), tone: 'muted' })
    } else {
      if (device.role === 'parent') {
        badges.push({ label: c.agentDevicesRoleParent, tone: 'active' })
      }
      if (device.stale) {
        badges.push({ label: c.agentDevicesStale, tone: 'warn' })
      }
    }
    return {
      id: device.id,
      title: device.name,
      subtitle: `${device.tokenHint}… · ${format(c.agentDevicesAdded, { time: when(device.createdAt) })}`,
      meta: device.lastSeenAt
        ? format(c.agentDevicesLastSynced, { time: when(device.lastSeenAt) })
        : c.agentDevicesNeverSynced,
      badges,
      actions: device.revokedAt
        ? []
        : [
            device.role === 'parent'
              ? {
                  key: 'site-parent',
                  label: c.agentDevicesMakeSiteParent,
                  onClick: () => void setRole(device, 'child'),
                  disabled: working,
                  variant: 'secondary' as const,
                }
              : {
                  key: 'make-parent',
                  label: c.agentDevicesMakeParent,
                  onClick: () => void setRole(device, 'parent'),
                  disabled: working,
                  variant: 'secondary' as const,
                  confirm: format(c.agentDevicesMakeParentConfirm, {
                    name: device.name,
                  }),
                },
            {
              key: 'remove',
              label: c.agentDevicesRemove,
              onClick: () => void remove(device),
              disabled: working,
              variant: 'danger' as const,
              confirm: format(c.agentDevicesRemoveConfirm, {
                name: device.name,
              }),
            },
          ],
    }
  })

  const form = (
    <div className="agent-devices-form">
      <InputItem
        itemKey="agent_device_name"
        label={c.agentDevicesName}
        value={name}
        onChange={setName}
        placeholder={c.agentDevicesNamePlaceholder}
        inputType="text"
        layout="vertical"
      />
      <SettingsButton
        variant="primary"
        size="sm"
        icon={<LuPlus size={14} />}
        loading={working}
        disabled={working || !name.trim()}
        onClick={() => void add()}
      >
        {c.agentDevicesCreate}
      </SettingsButton>
    </div>
  )

  return (
    <SettingGroup
      title={c.agentDevicesTitle}
      icon={<LuMonitor />}
      description={c.agentDevicesDesc}
    >
      {issued ? (
        <InfoActionCard
          title={format(c.agentDevicesTokenTitle, { name: issued.name })}
          tone="info"
          fields={[
            {
              key: 'token',
              label: c.agentDevicesTokenLabel,
              value: issued.token,
              mono: true,
              copyable: true,
            },
          ]}
          actions={[
            {
              key: 'done',
              label: c.agentDevicesTokenDone,
              onClick: () => setIssued(null),
              variant: 'secondary',
            },
          ]}
        >
          <p className="info-action-card-lede">{c.agentDevicesTokenOnce}</p>
        </InfoActionCard>
      ) : null}
      <ManagedList
        stats={stats}
        formTitle={c.agentDevicesAdd}
        formDescription={c.agentDevicesAddDesc}
        formIcon={<LuPlus aria-hidden />}
        formCollapseLabel={t.common.cancel}
        formOpen={formOpen}
        onFormOpenChange={setFormOpen}
        form={form}
        items={items}
        emptyText={loading ? t.common.loading : c.agentDevicesEmpty}
        loading={loading && list === null}
        working={working}
        maxHeight={null}
      />
      <p className="setting-hint">{c.agentDevicesParentHint}</p>
    </SettingGroup>
  )
}

export default AgentDevicesGroup
