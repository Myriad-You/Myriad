import type {
  HostImport,
  HostLease,
  HostMove,
  SyncDevice,
  SyncDeviceList,
} from '../../services/agent/agentApi'
import type { ManagedListItem, ManagedListStat } from '../settings/ManagedList'
import { LuDownload, LuMonitor, LuPlus, LuUpload } from '@lib/icons'

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
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

/** Devices (a Mac) connected to her on this site. Only the site owner sees it. */
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
  const [exportOpen, setExportOpen] = useState(false)
  const [passphrase, setPassphrase] = useState('')
  const [again, setAgain] = useState('')
  const [importOpen, setImportOpen] = useState(false)
  const [importFile, setImportFile] = useState<File | null>(null)
  const [importPassphrase, setImportPassphrase] = useState('')
  const [importProgress, setImportProgress] = useState<string | null>(null)
  const [landedNote, setLandedNote] = useState<string | null>(null)
  const [mergePass, setMergePass] = useState('')
  const [mergeAgain, setMergeAgain] = useState('')
  const [movePass, setMovePass] = useState('')
  const [moveAgain, setMoveAgain] = useState('')
  const fileInput = useRef<HTMLInputElement>(null)

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

  const forget = useCallback(
    async (device: SyncDevice) => {
      setWorking(true)
      try {
        setList(await agentService.forgetSyncDevice(device.id))
      } catch (error) {
        fail(error, c.agentDevicesForgetFailed)
      } finally {
        setWorking(false)
      }
    },
    [c.agentDevicesForgetFailed, fail],
  )

  // A move of her: confirmed (she freezes) or called off (she is back as
  // she was). Only here, logged in; a device token cannot do either.
  const moveAction = useCallback(
    async (action: 'confirm' | 'cancel', move: HostMove) => {
      setWorking(true)
      try {
        const toDevice = move.kind === 'move' && move.from === 'site'
        const { host } =
          action === 'confirm'
            ? await agentService.confirmHostMove(
                move.id,
                toDevice ? movePass : undefined,
              )
            : await agentService.cancelHostMove(move.id)
        setList((current) => (current ? { ...current, host } : current))
        setMovePass('')
        setMoveAgain('')
        await load()
      } catch (error) {
        fail(error, c.agentHostActionFailed)
        await load()
      } finally {
        setWorking(false)
      }
    },
    [c.agentHostActionFailed, fail, load, movePass],
  )

  // 选主机 (D10): she goes to live on a device (the device agrees, then the
  // owner confirms here; Q8: open with H4, the acceptance entry before), or
  // comes back from the device she lives on.
  const startMove = useCallback(
    async (kind: 'move' | 'back', device: string) => {
      setWorking(true)
      try {
        await agentService.startHostMove(kind, device)
        await load()
      } catch (error) {
        fail(error, c.agentHostActionFailed)
        await load()
      } finally {
        setWorking(false)
      }
    },
    [c.agentHostActionFailed, fail, load],
  )

  // 「导出并搬走」 on the site (Q7): the file, and she moves out of the site
  // until the owner lets this copy go or undoes it.
  const moveOut = useCallback(async () => {
    if (!passphrase || passphrase !== again) return
    setWorking(true)
    try {
      const blob = await agentService.moveHerOut(passphrase)
      const stamp = new Date()
        .toISOString()
        .slice(0, 16)
        .replace(/[-:]/g, '')
        .replace('T', '-')
      const url = URL.createObjectURL(blob)
      const anchor = document.createElement('a')
      anchor.href = url
      anchor.download = `her-${stamp}.her-archive`
      document.body.appendChild(anchor)
      anchor.click()
      document.body.removeChild(anchor)
      URL.revokeObjectURL(url)
      setPassphrase('')
      setAgain('')
      setExportOpen(false)
    } catch (error) {
      fail(error, c.agentHostExportFailed)
    } finally {
      setWorking(false)
      await load()
    }
  }, [again, c.agentHostExportFailed, fail, load, passphrase])

  // What became of the site she moved out of, and what is left of others
  // while she is away (Q2): undone, let go, or cleared, here only.
  const leaveAction = useCallback(
    async (action: 'undo' | 'letGo' | 'clearSealed') => {
      setWorking(true)
      try {
        setList(
          action === 'undo'
            ? await agentService.undoMoveOut()
            : action === 'letGo'
              ? await agentService.letHerGo()
              : await agentService.clearSealed(),
        )
      } catch (error) {
        fail(error, c.agentHostActionFailed)
        await load()
      } finally {
        setWorking(false)
      }
    },
    [c.agentHostActionFailed, fail, load],
  )

  const backupAction = useCallback(
    async (action: 'download' | 'delete', id: string, day: string) => {
      setWorking(true)
      try {
        if (action === 'delete') {
          setList(await agentService.deleteBackup(id))
          return
        }
        const blob = await agentService.downloadBackup(id)
        const url = URL.createObjectURL(blob)
        const anchor = document.createElement('a')
        anchor.href = url
        anchor.download = `her-backup-${day}.her-archive`
        document.body.appendChild(anchor)
        anchor.click()
        document.body.removeChild(anchor)
        URL.revokeObjectURL(url)
      } catch (error) {
        fail(error, c.agentDevicesLoadFailed)
      } finally {
        setWorking(false)
      }
    },
    [c.agentDevicesLoadFailed, fail],
  )

  // 「导出她」: always sealed with the passphrase typed here, used once by
  // the site for the key and kept nowhere (H3-6, H3b-3).
  const exportHer = useCallback(async () => {
    if (!passphrase || passphrase !== again) return
    setWorking(true)
    try {
      const blob = await agentService.exportHer(passphrase)
      const stamp = new Date()
        .toISOString()
        .slice(0, 16)
        .replace(/[-:]/g, '')
        .replace('T', '-')
      const url = URL.createObjectURL(blob)
      const anchor = document.createElement('a')
      anchor.href = url
      anchor.download = `her-${stamp}.her-archive`
      document.body.appendChild(anchor)
      anchor.click()
      document.body.removeChild(anchor)
      URL.revokeObjectURL(url)
      setPassphrase('')
      setAgain('')
      setExportOpen(false)
    } catch (error) {
      fail(error, c.agentHostExportFailed)
    } finally {
      setWorking(false)
    }
  }, [again, c.agentHostExportFailed, fail, passphrase])

  // Her coming into this site, which has none (H3c): a device's request is
  // confirmed or turned down here, logged in; one under way can be called
  // off, and the site is as it was.
  const importAction = useCallback(
    async (action: 'confirm' | 'cancel', pending: HostImport) => {
      setWorking(true)
      try {
        if (action === 'cancel') {
          setList(await agentService.cancelHostImport(pending.id))
          return
        }
        try {
          setList(await agentService.confirmHostImport(pending.id))
        } catch (error) {
          // A site she left keeps what was left of another her (H3e-15).
          if (
            error instanceof ApiError &&
            error.code === 'import_replaces_sealed' &&
            window.confirm(c.agentHostImportReplaceConfirm)
          ) {
            setList(
              await agentService.confirmHostImport(pending.id, undefined, true),
            )
            return
          }
          throw error
        }
      } catch (error) {
        fail(error, c.agentHostImportFailed)
        await load()
      } finally {
        setWorking(false)
      }
    },
    [c.agentHostImportFailed, c.agentHostImportReplaceConfirm, fail, load],
  )

  // A device's her merged into the site's (H3d): confirmed here, logged in,
  // with a passphrase the site seals its own her with first (used once).
  const mergeAction = useCallback(
    async (action: 'confirm' | 'cancel', pending: HostImport) => {
      setWorking(true)
      try {
        setList(
          action === 'confirm'
            ? await agentService.confirmHostImport(pending.id, mergePass)
            : await agentService.cancelHostImport(pending.id),
        )
        setMergePass('')
        setMergeAgain('')
      } catch (error) {
        fail(error, c.agentHostMergeFailed)
        await load()
      } finally {
        setWorking(false)
      }
    },
    [c.agentHostMergeFailed, fail, load, mergePass],
  )

  const retiredAction = useCallback(
    async (action: 'download' | 'delete', name: string) => {
      setWorking(true)
      try {
        if (action === 'delete') {
          setList(await agentService.deleteRetired(name))
          return
        }
        const blob = await agentService.downloadRetired(name)
        const url = URL.createObjectURL(blob)
        const anchor = document.createElement('a')
        anchor.href = url
        anchor.download = name
        document.body.appendChild(anchor)
        anchor.click()
        document.body.removeChild(anchor)
        URL.revokeObjectURL(url)
      } catch (error) {
        fail(error, c.agentDevicesLoadFailed)
      } finally {
        setWorking(false)
      }
    },
    [c.agentDevicesLoadFailed, fail],
  )

  // A persona field by name, as the owner reads it: her visual keys under
  // her look, a key only some end knows as it is.
  const mergeFieldLabel = useCallback(
    (field: string) => {
      const labels = c as unknown as Record<string, string | undefined>
      if (field.startsWith('visual.')) {
        return `${labels.agentHostMergeField_visual ?? 'visual'} · ${field.slice(7)}`
      }
      return labels[`agentHostMergeField_${field}`] ?? field
    },
    [c],
  )

  const countsLine = useCallback(
    (counts: Record<string, number>) =>
      format(c.agentHostImportCounts, {
        n: Object.values(counts).reduce((sum, n) => sum + n, 0),
        memory: counts.memory ?? 0,
        diary: counts.diary ?? 0,
        conversation: counts.conversation ?? 0,
        outfit: counts.outfit ?? 0,
      }),
    [c.agentHostImportCounts, format],
  )

  // From an export file: sent in pieces (an interrupted upload of the same
  // file goes on where it stopped), then opened here once with the
  // passphrase, which the site uses for the key and keeps nowhere.
  const importFromFile = useCallback(async () => {
    const file = importFile
    if (!file || !importPassphrase) return
    setWorking(true)
    setLandedNote(null)
    const mb = (bytes: number) => `${(bytes / 1048576).toFixed(1)} MB`
    try {
      const pending = list?.import
      let session: HostImport
      let piece = 8 * 1024 * 1024
      if (
        pending &&
        pending.source === 'file' &&
        pending.phase === 'uploading' &&
        pending.total === file.size
      ) {
        session = pending
      } else {
        const begun = await agentService.beginFileImport(file.size)
        session = begun.import
        piece = begun.pieceBytes
      }
      let offset = session.received
      while (offset < file.size) {
        setImportProgress(
          format(c.agentHostImportFileUploading, {
            done: mb(offset),
            total: mb(file.size),
          }),
        )
        const sent = await agentService.sendFileImportPiece(
          session.id,
          offset,
          file.slice(offset, Math.min(file.size, offset + piece)),
        )
        offset = sent.import.received
      }
      setImportProgress(c.agentHostImportFileOpening)
      let landed
      try {
        landed = await agentService.openFileImport(session.id, importPassphrase)
      } catch (error) {
        // A site she left keeps what was left of another her (H3e-15).
        if (
          error instanceof ApiError &&
          error.code === 'import_replaces_sealed' &&
          window.confirm(c.agentHostImportReplaceConfirm)
        ) {
          landed = await agentService.openFileImport(
            session.id,
            importPassphrase,
            true,
          )
        } else {
          throw error
        }
      }
      setImportPassphrase('')
      setImportFile(null)
      setImportOpen(false)
      setLandedNote(
        format(c.agentHostImportDone, { counts: countsLine(landed.counts) }) +
          (landed.herOn ? '' : ` ${c.agentHostImportHerOff}`),
      )
    } catch (error) {
      fail(error, c.agentHostImportFailed)
    } finally {
      setImportProgress(null)
      setWorking(false)
      await load()
    }
  }, [
    c,
    countsLine,
    fail,
    format,
    importFile,
    importPassphrase,
    list?.import,
    load,
  ])

  const clearHeld = useCallback(async () => {
    setWorking(true)
    try {
      await agentService.clearHeld()
      await load()
    } catch (error) {
      fail(error, c.agentHostHeldClearFailed)
    } finally {
      setWorking(false)
    }
  }, [c.agentHostHeldClearFailed, fail, load])

  const when = useCallback(
    (at: string) => new Date(at).toLocaleString(locale || undefined),
    [locale],
  )

  const devices = useMemo(() => list?.devices ?? [], [list])
  const live = devices.filter((device) => !device.revokedAt)

  const host: HostLease | null = list?.host ?? null
  const pending: HostImport | null = list?.import ?? null
  // A move that is an import (or a reclaim) has its own card below.
  const move =
    host?.move &&
    !host.move.id.startsWith('imp_') &&
    host.move.kind !== 'recover'
      ? host.move
      : null
  const moveToDevice = move !== null && move.kind === 'move' && move.from === 'site'
  const moveBack = move !== null && move.kind === 'move' && move.to === 'site'
  const deviceName = (id: string, name: string | null) =>
    id === 'site' ? c.agentHostSite : (name ?? c.agentHostUnknownDevice)
  // "10 min 27 s", in the common duration words.
  const clockAmount = (ms: number) => {
    const sec = Math.round(ms / 1000)
    if (sec < 60) return format(t.common.durationSeconds, { sec })
    const min = Math.floor(sec / 60)
    return sec % 60
      ? format(t.common.durationMinutesSeconds, { min, sec: sec % 60 })
      : format(t.common.durationMinutes, { min })
  }
  // Her copy here is her only while she lives here or is moving (H3b-5).
  const exportable =
    host !== null &&
    (host.state === 'frozen' ||
      host.state === 'moved_out' ||
      (host.state === 'live' && host.holder === 'site'))
  const livesElsewhere =
    host !== null && host.state === 'live' && host.holder !== 'site'
  // She lives here and nothing is under way: she can be given another host.
  const choosable = host !== null && host.state === 'live' && host.holder === 'site' && !host.move

  // Choosing a host here comes later (H4); for now this says where she
  // lives, and a move a device asked for is confirmed or turned down here.
  const stats: ManagedListStat[] = [
    {
      key: 'live',
      label: c.agentDevicesStatLive,
      value: `${live.length} / ${list?.limit ?? 10}`,
      tone: live.length === 0 ? 'muted' : 'default',
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
    if (!device.revokedAt && host?.holder === device.id) {
      badges.push({ label: c.agentDevicesHostBadge, tone: 'default' })
    }
    if (
      !device.revokedAt &&
      move &&
      (move.from === device.id || move.to === device.id)
    ) {
      badges.push({
        label:
          move.phase === 'proposed'
            ? c.agentDevicesMoveAsked
            : c.agentDevicesMoving,
        tone: 'warn',
      })
    }
    if (device.revokedAt) {
      badges.push({ label: removedWhy(device), tone: 'muted' })
    } else if (device.stale && !device.connected) {
      badges.push({ label: c.agentDevicesStale, tone: 'warn' })
    }
    return {
      id: device.id,
      title: device.name,
      subtitle: `${device.tokenHint}… · ${format(c.agentDevicesAdded, { time: when(device.createdAt) })}`,
      // Connected now, or when it last was: her link records it as it opens.
      meta: device.connected
        ? c.agentDevicesConnected
        : device.lastSeenAt
          ? format(c.agentDevicesLastConnected, {
              time: when(device.lastSeenAt),
            })
          : c.agentDevicesNeverConnected,
      badges,
      actions: device.revokedAt
        ? [
            {
              key: 'forget',
              label: c.agentDevicesForget,
              onClick: () => void forget(device),
              disabled: working,
              variant: 'danger' as const,
              confirm: format(c.agentDevicesForgetConfirm, {
                name: device.name,
              }),
            },
          ]
        : [
            {
              key: 'remove',
              label: c.agentDevicesRemove,
              onClick: () => void remove(device),
              disabled: working,
              variant: 'danger' as const,
              // She lives on it: removing it leaves her there, on her own (H3e-14).
              confirm: format(
                host?.holder === device.id && host.state !== 'vacant'
                  ? c.agentDevicesRemoveHostConfirm
                  : c.agentDevicesRemoveConfirm,
                { name: device.name },
              ),
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

  // Only when the site could not correct its clock: no trusted measurement
  // to correct it by, or one too far off to believe.
  const clock = list?.serverClock
  const clockWarning: { title: string; body: string } | null = (() => {
    if (!clock || clock.corrected) return null
    const offset = clock.offsetMs
    if (clock.trusted && offset !== null && Math.abs(offset) > clock.maxCorrectionMs) {
      return {
        title: format(
          offset < 0
            ? c.agentServerClockBehindTitle
            : c.agentServerClockAheadTitle,
          { amount: clockAmount(Math.abs(offset)) },
        ),
        body: c.agentServerClockRangeBody,
      }
    }
    return {
      title: c.agentServerClockUnmeasuredTitle,
      body: c.agentServerClockUnmeasuredBody,
    }
  })()

  return (
    <SettingGroup
      title={c.agentDevicesTitle}
      icon={<LuMonitor />}
      description={c.agentDevicesDesc}
    >
      {host ? (
        <p className="setting-hint">
          {host.state === 'vacant'
            ? c.agentHostVacant
            : format(c.agentHostLivesOn, {
                where: deviceName(host.holder, host.holderName),
                lineage: host.lineage.slice(0, 8),
                epoch: host.epoch,
              })}
        </p>
      ) : null}
      {clockWarning ? (
        <InfoActionCard title={clockWarning.title} tone="warn">
          <p className="info-action-card-lede">{clockWarning.body}</p>
        </InfoActionCard>
      ) : null}
      {move && move.phase === 'proposed' && moveToDevice ? (
        <InfoActionCard
          title={format(c.agentHostToDeviceTitle, {
            name: deviceName(move.to, move.toName),
          })}
          tone="info"
          actions={[
            ...(move.agreed
              ? [
                  {
                    key: 'confirm',
                    label: c.agentHostConfirm,
                    onClick: () => void moveAction('confirm', move),
                    disabled:
                      working ||
                      !list?.deviceHostOpen ||
                      !movePass ||
                      movePass !== moveAgain,
                    variant: 'primary' as const,
                    confirm: c.agentHostToDeviceConfirmConfirm,
                  },
                ]
              : []),
            {
              key: 'decline',
              label: c.agentHostDecline,
              onClick: () => void moveAction('cancel', move),
              disabled: working,
              variant: 'secondary',
            },
          ]}
        >
          {move.agreed ? (
            <>
              <p className="info-action-card-lede">
                {format(c.agentHostToDeviceBody, {
                  name: deviceName(move.to, move.toName),
                })}
              </p>
              <InputItem
                itemKey="agent_move_passphrase"
                label={c.agentHostToDevicePassphrase}
                value={movePass}
                onChange={setMovePass}
                inputType="password"
                autoComplete="new-password"
                layout="vertical"
              />
              <InputItem
                itemKey="agent_move_again"
                label={c.agentHostExportAgain}
                value={moveAgain}
                onChange={setMoveAgain}
                inputType="password"
                autoComplete="new-password"
                layout="vertical"
              />
              {moveAgain && movePass !== moveAgain ? (
                <p className="setting-hint">{c.agentHostExportMismatch}</p>
              ) : null}
            </>
          ) : (
            <p className="info-action-card-lede">
              {format(c.agentHostToDeviceWaiting, {
                name: deviceName(move.to, move.toName),
              })}
            </p>
          )}
        </InfoActionCard>
      ) : move && move.phase === 'proposed' && moveBack ? (
        <InfoActionCard
          title={format(c.agentHostBackTitle, {
            name: deviceName(move.from, move.fromName),
          })}
          tone="info"
          actions={[
            {
              key: 'confirm',
              label: c.agentHostConfirm,
              onClick: () => void moveAction('confirm', move),
              disabled: working,
              variant: 'primary',
              confirm: c.agentHostConfirmConfirm,
            },
            {
              key: 'decline',
              label: c.agentHostDecline,
              onClick: () => void moveAction('cancel', move),
              disabled: working,
              variant: 'secondary',
            },
          ]}
        >
          <p className="info-action-card-lede">{c.agentHostBackBody}</p>
        </InfoActionCard>
      ) : null}
      {move && move.phase === 'proposed' && !moveToDevice && !moveBack ? (
        <InfoActionCard
          title={format(c.agentHostProposedTitle, {
            name: deviceName(
              move.kind === 'merge' ? move.from : move.to,
              move.kind === 'merge' ? move.fromName : move.toName,
            ),
            what:
              move.kind === 'merge'
                ? c.agentHostKindMerge
                : c.agentHostKindMove,
          })}
          tone="info"
          actions={[
            {
              key: 'confirm',
              label: c.agentHostConfirm,
              onClick: () => void moveAction('confirm', move),
              disabled: working,
              variant: 'primary',
              confirm: c.agentHostConfirmConfirm,
            },
            {
              key: 'decline',
              label: c.agentHostDecline,
              onClick: () => void moveAction('cancel', move),
              disabled: working,
              variant: 'secondary',
            },
          ]}
        >
          <p className="info-action-card-lede">{c.agentHostProposedBody}</p>
        </InfoActionCard>
      ) : null}
      {move && move.phase !== 'proposed' ? (
        <InfoActionCard
          title={format(c.agentHostFrozenTitle, { step: move.step })}
          tone="warn"
          actions={[
            {
              key: 'cancel',
              label: c.agentHostCancel,
              onClick: () => void moveAction('cancel', move),
              disabled:
                working ||
                move.phase === 'committed' ||
                move.phase === 'retired',
              variant: 'secondary',
              confirm: c.agentHostCancelConfirm,
            },
          ]}
        >
          <p className="info-action-card-lede">
            {format(c.agentHostFrozenBody, {
              what:
                move.kind === 'merge'
                  ? c.agentHostKindMerge
                  : c.agentHostKindMove,
              name: deviceName(
                move.kind === 'merge' || moveBack ? move.from : move.to,
                move.kind === 'merge' || moveBack ? move.fromName : move.toName,
              ),
            })}
          </p>
        </InfoActionCard>
      ) : null}
      {pending && pending.source === 'device' && pending.kind === 'recover' ? (
        <InfoActionCard
          title={format(
            pending.phase === 'proposed'
              ? c.agentHostRecoverTitle
              : c.agentHostRecoveringTitle,
            { device: pending.deviceName ?? c.agentHostUnknownDevice },
          )}
          tone="warn"
          actions={[
            ...(pending.phase === 'proposed'
              ? [
                  {
                    key: 'confirm',
                    label: c.agentHostRecoverConfirm,
                    onClick: () => void importAction('confirm', pending),
                    disabled: working,
                    variant: 'primary' as const,
                    confirm: c.agentHostRecoverConfirmConfirm,
                  },
                ]
              : []),
            {
              key: 'decline',
              label:
                pending.phase === 'proposed'
                  ? c.agentHostRecoverDecline
                  : c.agentHostCancel,
              onClick: () => void importAction('cancel', pending),
              disabled: working,
              variant: 'secondary',
            },
          ]}
        >
          <p className="info-action-card-lede">
            {format(c.agentHostRecoverBody, {
              device: pending.deviceName ?? c.agentHostUnknownDevice,
              from: host ? deviceName(host.holder, host.holderName) : '—',
            })}
          </p>
          <p className="setting-hint">{countsLine(pending.counts)}</p>
        </InfoActionCard>
      ) : null}
      {host && host.state === 'moved_out' ? (
        <InfoActionCard
          title={c.agentHostMovedOutTitle}
          tone="warn"
          actions={[
            {
              key: 'undo',
              label: c.agentHostMovedOutUndo,
              onClick: () => void leaveAction('undo'),
              disabled: working,
              variant: 'secondary',
            },
            {
              key: 'letGo',
              label: c.agentHostMovedOutLetGo,
              onClick: () => void leaveAction('letGo'),
              disabled: working,
              variant: 'danger',
              confirm: c.agentHostMovedOutLetGoConfirm,
            },
          ]}
        >
          <p className="info-action-card-lede">{c.agentHostMovedOutBody}</p>
        </InfoActionCard>
      ) : null}
      {choosable || livesElsewhere ? (
        <InfoActionCard
          title={c.agentHostChooseTitle}
          tone="info"
          actions={
            livesElsewhere && host
              ? [
                  {
                    key: 'back',
                    label: c.agentHostChooseBack,
                    onClick: () => void startMove('back', host.holder),
                    disabled: working,
                    variant: 'secondary',
                    confirm: format(c.agentHostChooseBackConfirm, {
                      name: deviceName(host.holder, host.holderName),
                    }),
                  },
                ]
              : live.map((device) => ({
                  key: device.id,
                  label: format(c.agentHostChooseDevice, { name: device.name }),
                  onClick: () => void startMove('move', device.id),
                  disabled: working || !list?.deviceHostOpen,
                  variant: 'secondary' as const,
                }))
          }
        >
          <p className="info-action-card-lede">
            {list?.deviceHostOpen
              ? c.agentHostChooseOpenBody
              : c.agentHostChooseBody}
          </p>
        </InfoActionCard>
      ) : null}
      {list?.sealed != null ? (
        <InfoActionCard
          title={format(c.agentHostSealedTitle, { n: list.sealed })}
          tone="info"
          actions={[
            {
              key: 'clear',
              label: c.agentHostSealedClear,
              onClick: () => void leaveAction('clearSealed'),
              disabled: working,
              variant: 'secondary',
              confirm: c.agentHostSealedClearConfirm,
            },
          ]}
        >
          <p className="info-action-card-lede">
            {host?.state === 'vacant'
              ? c.agentHostSealedVacantBody
              : c.agentHostSealedBody}
          </p>
        </InfoActionCard>
      ) : null}
      {(list?.backups ?? []).length > 0 ? (
        <InfoActionCard
          title={c.agentHostBackupsTitle}
          tone="info"
        >
          <p className="info-action-card-lede">{c.agentHostBackupsBody}</p>
          {(list?.backups ?? []).map((backup) => (
            <div className="agent-devices-form" key={backup.id}>
              <span className="setting-hint">
                {format(c.agentHostBackupLine, {
                  day: backup.day,
                  device: backup.deviceName ?? c.agentHostUnknownDevice,
                  epoch: backup.epoch,
                  size: `${(backup.bytes / 1048576).toFixed(1)} MB`,
                })}
              </span>
              <SettingsButton
                variant="secondary"
                size="sm"
                icon={<LuDownload size={14} />}
                disabled={working}
                onClick={() => void backupAction('download', backup.id, backup.day)}
              >
                {c.agentHostBackupDownload}
              </SettingsButton>
              <SettingsButton
                variant="secondary"
                size="sm"
                disabled={working}
                onClick={() => {
                  if (window.confirm(c.agentHostBackupDeleteConfirm)) {
                    void backupAction('delete', backup.id, backup.day)
                  }
                }}
              >
                {c.agentHostBackupDelete}
              </SettingsButton>
            </div>
          ))}
        </InfoActionCard>
      ) : null}
      {pending && pending.source === 'device' && pending.kind === 'merge' ? (
        pending.phase === 'proposed' ? (
          <InfoActionCard
            title={format(c.agentHostMergeProposedTitle, {
              device: pending.deviceName ?? c.agentHostUnknownDevice,
              name: pending.name ?? '—',
            })}
            tone="info"
            actions={[
              {
                key: 'confirm',
                label: c.agentHostMergeConfirm,
                onClick: () => void mergeAction('confirm', pending),
                disabled: working || !mergePass || mergePass !== mergeAgain,
                variant: 'primary',
                confirm: c.agentHostMergeConfirmConfirm,
              },
              {
                key: 'decline',
                label: c.agentHostMergeDecline,
                onClick: () => void mergeAction('cancel', pending),
                disabled: working,
                variant: 'secondary',
                confirm: c.agentHostMergeDeclineConfirm,
              },
            ]}
          >
            {pending.merge && !pending.merge.unreadable ? (
              <>
                <p className="setting-hint">
                  {pending.merge.sameName
                    ? format(c.agentHostMergeSameName, {
                        name: pending.merge.names.site ?? '—',
                      })
                    : pending.subject === 'device'
                      ? format(c.agentHostMergeSubjectDevice, {
                          device: pending.deviceName ?? c.agentHostUnknownDevice,
                          name: pending.merge.names.device ?? '—',
                        })
                      : format(c.agentHostMergeSubjectSite, {
                          site: pending.merge.names.site ?? '—',
                        })}
                  {pending.merge.sameLineage
                    ? ` ${c.agentHostMergeSameLineage}`
                    : ''}
                </p>
                <ul className="setting-hint">
                  {pending.merge.fields.map((outcome) => (
                    <li key={outcome.field}>
                      {mergeFieldLabel(outcome.field)}：
                      {outcome.both
                        ? c.agentHostMergeFromBoth
                        : outcome.from === 'host'
                          ? c.agentHostMergeFromSite
                          : outcome.from === 'other'
                            ? c.agentHostMergeFromDevice
                            : c.agentHostMergeUnset}
                      {outcome.toHistory ? c.agentHostMergeToHistory : ''}
                    </li>
                  ))}
                </ul>
                {pending.merge.renamed.map((renamed) => (
                  <p className="setting-hint" key={`${renamed.side}:${renamed.from}`}>
                    {format(c.agentHostMergeRenamed, {
                      from: renamed.from,
                      to: renamed.to,
                      side:
                        renamed.side === 'host'
                          ? c.agentHostMergeSideSite
                          : c.agentHostMergeSideDevice,
                    })}
                  </p>
                ))}
                <p className="setting-hint">
                  {format(c.agentHostMergeCounts, {
                    counts: countsLine(pending.counts),
                    memories: pending.merge.site.memories,
                    diary: pending.merge.site.diary,
                  })}
                </p>
              </>
            ) : (
              <p className="setting-hint">{countsLine(pending.counts)}</p>
            )}
            <p className="info-action-card-lede">{c.agentHostMergeBody}</p>
            <InputItem
              itemKey="agent_merge_passphrase"
              label={c.agentHostMergePassphrase}
              value={mergePass}
              onChange={setMergePass}
              inputType="password"
              autoComplete="off"
              layout="vertical"
            />
            <InputItem
              itemKey="agent_merge_again"
              label={c.agentHostMergeAgain}
              value={mergeAgain}
              onChange={setMergeAgain}
              inputType="password"
              autoComplete="off"
              layout="vertical"
            />
            {mergeAgain && mergePass !== mergeAgain ? (
              <p className="setting-hint">{c.agentHostExportMismatch}</p>
            ) : null}
          </InfoActionCard>
        ) : (
          <InfoActionCard
            title={format(c.agentHostMergingTitle, {
              device: pending.deviceName ?? c.agentHostUnknownDevice,
            })}
            tone="info"
            actions={[
              {
                key: 'cancel',
                label: c.agentHostMergeCancel,
                onClick: () => void mergeAction('cancel', pending),
                disabled: working,
                variant: 'secondary',
                confirm: c.agentHostMergeCancelConfirm,
              },
            ]}
          >
            <p className="info-action-card-lede">{c.agentHostMergingBody}</p>
            <p className="setting-hint">{countsLine(pending.counts)}</p>
          </InfoActionCard>
        )
      ) : null}
      {(list?.retired ?? []).map((file, index) => (
        <InfoActionCard
          key={file.name}
          title={`${c.agentHostRetiredTitle} · ${file.name}`}
          tone="info"
          actions={[
            {
              key: 'download',
              label: c.agentHostRetiredDownload,
              onClick: () => void retiredAction('download', file.name),
              disabled: working,
              variant: 'secondary',
            },
            {
              key: 'delete',
              label: c.agentHostRetiredDelete,
              onClick: () => void retiredAction('delete', file.name),
              disabled: working,
              variant: 'secondary',
              confirm: c.agentHostRetiredDeleteConfirm,
            },
          ]}
        >
          {index === 0 ? (
            <p className="info-action-card-lede">{c.agentHostRetiredBody}</p>
          ) : null}
          <p className="setting-hint">
            {(file.bytes / 1048576).toFixed(1)} MB
            {file.at ? ` · ${new Date(file.at).toLocaleString(locale || undefined)}` : ''}
          </p>
        </InfoActionCard>
      ))}
      {pending &&
      pending.source === 'device' &&
      (pending.kind ?? 'import') === 'import' ? (
        pending.phase === 'proposed' ? (
          <InfoActionCard
            title={format(c.agentHostImportProposedTitle, {
              device: pending.deviceName ?? c.agentHostUnknownDevice,
              name: pending.name ?? '—',
            })}
            tone="info"
            actions={[
              {
                key: 'confirm',
                label: c.agentHostImportConfirm,
                onClick: () => void importAction('confirm', pending),
                disabled: working,
                variant: 'primary',
                confirm: c.agentHostImportConfirmConfirm,
              },
              {
                key: 'decline',
                label: c.agentHostImportDecline,
                onClick: () => void importAction('cancel', pending),
                disabled: working,
                variant: 'secondary',
              },
            ]}
          >
            <p className="info-action-card-lede">
              {c.agentHostImportProposedBody}
            </p>
            <p className="setting-hint">{countsLine(pending.counts)}</p>
          </InfoActionCard>
        ) : (
          <InfoActionCard
            title={format(c.agentHostImportMovingTitle, {
              device: pending.deviceName ?? c.agentHostUnknownDevice,
            })}
            tone="warn"
            actions={[
              {
                key: 'cancel',
                label: c.agentHostImportCancel,
                onClick: () => void importAction('cancel', pending),
                disabled: working,
                variant: 'secondary',
                confirm: c.agentHostImportCancelConfirm,
              },
            ]}
          >
            <p className="info-action-card-lede">
              {c.agentHostImportMovingBody}
            </p>
            <p className="setting-hint">{countsLine(pending.counts)}</p>
          </InfoActionCard>
        )
      ) : null}
      {landedNote ? <p className="setting-hint">{landedNote}</p> : null}
      {list?.empty && !(pending && pending.source === 'device') ? (
        importOpen ? (
          <InfoActionCard
            title={c.agentHostImportFileTitle}
            tone="info"
            actions={[
              {
                key: 'import',
                label: c.agentHostImportFileGo,
                onClick: () => void importFromFile(),
                disabled: working || !importFile || !importPassphrase,
                variant: 'primary',
              },
              {
                key: 'close',
                label: t.common.cancel,
                onClick: () => {
                  setImportOpen(false)
                  setImportFile(null)
                  setImportPassphrase('')
                },
                disabled: working,
                variant: 'secondary',
              },
            ]}
          >
            <p className="info-action-card-lede">{c.agentHostImportFileBody}</p>
            <div className="agent-devices-form">
              <SettingsButton
                variant="secondary"
                size="sm"
                icon={<LuUpload size={14} />}
                disabled={working}
                onClick={() => fileInput.current?.click()}
              >
                {importFile ? importFile.name : c.agentHostImportFilePick}
              </SettingsButton>
              <input
                ref={fileInput}
                type="file"
                accept=".her-archive"
                className="hidden"
                title={c.agentHostImportFilePick}
                onChange={(event) =>
                  setImportFile(event.target.files?.[0] ?? null)
                }
              />
            </div>
            <InputItem
              itemKey="agent_import_passphrase"
              label={c.agentHostImportFilePassphrase}
              value={importPassphrase}
              onChange={setImportPassphrase}
              inputType="password"
              autoComplete="off"
              layout="vertical"
            />
            {importProgress ? (
              <p className="setting-hint">{importProgress}</p>
            ) : null}
          </InfoActionCard>
        ) : (
          <div className="agent-devices-form">
            <SettingsButton
              variant="secondary"
              size="sm"
              icon={<LuUpload size={14} />}
              disabled={working}
              onClick={() => setImportOpen(true)}
            >
              {c.agentHostImportFile}
            </SettingsButton>
          </div>
        )
      ) : null}
      {list?.held ? (
        <InfoActionCard
          title={format(c.agentHostHeldTitle, { n: list.held })}
          tone="info"
          actions={[
            {
              key: 'clear',
              label: c.agentHostHeldClear,
              onClick: () => void clearHeld(),
              disabled: working,
              variant: 'secondary',
              confirm: c.agentHostHeldClearConfirm,
            },
          ]}
        >
          <p className="info-action-card-lede">{c.agentHostHeldBody}</p>
        </InfoActionCard>
      ) : null}
      {exportable ? (
        exportOpen ? (
          <InfoActionCard
            title={c.agentHostExportTitle}
            tone="info"
            actions={[
              {
                key: 'export',
                label: c.agentHostExportGo,
                onClick: () => void exportHer(),
                disabled: working || !passphrase || passphrase !== again,
                variant: 'primary',
              },
              ...(host && host.state === 'live' && host.holder === 'site'
                ? [
                    {
                      key: 'moveOut',
                      label: c.agentHostExportMoveOut,
                      onClick: () => void moveOut(),
                      disabled: working || !passphrase || passphrase !== again,
                      variant: 'secondary' as const,
                      confirm: c.agentHostExportMoveOutConfirm,
                    },
                  ]
                : []),
              {
                key: 'close',
                label: t.common.cancel,
                onClick: () => {
                  setExportOpen(false)
                  setPassphrase('')
                  setAgain('')
                },
                disabled: working,
                variant: 'secondary',
              },
            ]}
          >
            <p className="info-action-card-lede">{c.agentHostExportBody}</p>
            <InputItem
              itemKey="agent_export_passphrase"
              label={c.agentHostExportPassphrase}
              value={passphrase}
              onChange={setPassphrase}
              inputType="password"
              autoComplete="new-password"
              layout="vertical"
            />
            <InputItem
              itemKey="agent_export_passphrase_again"
              label={c.agentHostExportAgain}
              value={again}
              onChange={setAgain}
              inputType="password"
              autoComplete="new-password"
              layout="vertical"
            />
            {again && passphrase !== again ? (
              <p className="setting-hint">{c.agentHostExportMismatch}</p>
            ) : null}
          </InfoActionCard>
        ) : (
          <div className="agent-devices-form">
            <SettingsButton
              variant="secondary"
              size="sm"
              icon={<LuDownload size={14} />}
              disabled={working}
              onClick={() => setExportOpen(true)}
            >
              {c.agentHostExport}
            </SettingsButton>
          </div>
        )
      ) : livesElsewhere && host ? (
        <p className="setting-hint">
          {format(c.agentHostExportElsewhere, {
            where: deviceName(host.holder, host.holderName),
          })}
        </p>
      ) : null}
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
    </SettingGroup>
  )
}

export default AgentDevicesGroup
