import { LuCopy } from '@lib/icons'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'
import type { QqPairingStatus } from '../../services/agent/agentApi'
import { userFacingError } from '../../utils/userFacingError'
import { pairingCodeLive } from './channelPairing'
import './ChannelPairingPanel.css'

export type ChannelPairingKind = 'qq' | 'telegram' | 'discord_dm' | 'feishu'

export function ChannelPairingPanel({
  channel,
  openHref,
  receiveReady = true,
}: {
  channel: ChannelPairingKind
  openHref?: string | null
  receiveReady?: boolean
}) {
  const { t, locale, format } = useI18n()
  const copy = useMemo(() => pairingCopy(t, channel), [t, channel])
  const [pairing, setPairing] = useState<QqPairingStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [copied, setCopied] = useState(false)
  const [error, setError] = useState('')

  const load = useCallback(async () => {
    try {
      const data =
        channel === 'qq'
          ? await agentService.getQqPairing()
          : channel === 'telegram'
            ? await agentService.getTelegramPairing()
            : channel === 'feishu'
              ? await agentService.getFeishuPairing()
              : await agentService.getDiscordPairing()
      setPairing(data.pairing)
    } catch (err) {
      setError(userFacingError(err, copy.loadFailed))
    }
  }, [channel, copy.loadFailed])

  useEffect(() => {
    void load()
  }, [load])

  useEffect(() => {
    if (pairing?.paired) return
    const timer = window.setInterval(() => {
      void load()
    }, 3000)
    return () => window.clearInterval(timer)
  }, [load, pairing?.paired])

  const liveCode = pairingCodeLive(pairing?.pendingExpiresAt)
    ? pairing?.pendingCode
    : null
  const expiryLabel = useMemo(() => {
    if (!pairing?.pendingExpiresAt) return null
    const at = Date.parse(pairing.pendingExpiresAt)
    if (!Number.isFinite(at)) return null
    const time = new Date(at).toLocaleString(locale, {
      hour: '2-digit',
      minute: '2-digit',
    })
    return liveCode
      ? format(t.userModal.pairingExpiresAt, { time })
      : t.userModal.pairingExpired
  }, [format, liveCode, locale, pairing?.pendingExpiresAt, t.userModal])

  const handleIssue = async () => {
    setError('')
    setBusy(true)
    setCopied(false)
    try {
      const data =
        channel === 'qq'
          ? await agentService.issueQqPairingCode()
          : channel === 'telegram'
            ? await agentService.issueTelegramPairingCode()
            : channel === 'feishu'
              ? await agentService.issueFeishuPairingCode()
              : await agentService.issueDiscordPairingCode()
      setPairing(data.pairing)
    } catch (err) {
      setError(userFacingError(err, copy.issueFailed))
    } finally {
      setBusy(false)
    }
  }

  const handleCopy = async (code: string) => {
    try {
      await navigator.clipboard.writeText(code)
      setCopied(true)
    } catch {
      setError(copy.copyFailed)
    }
  }

  const handleUnpair = async () => {
    setError('')
    setBusy(true)
    try {
      if (channel === 'qq') {
        await agentService.unpairQq()
      } else if (channel === 'telegram') {
        await agentService.unpairTelegram()
      } else if (channel === 'feishu') {
        await agentService.unpairFeishu()
      } else {
        await agentService.unpairDiscord()
      }
      await load()
    } catch (err) {
      setError(userFacingError(err, copy.unpairFailed))
    } finally {
      setBusy(false)
    }
  }

  return (
    <section className="channel-pairing">
      <h4 className="channel-pairing-title">{copy.title}</h4>
      <p className="channel-pairing-hint">{copy.hint}</p>
      {pairing?.paired ? (
        <div className="channel-pairing-row">
          <span className="channel-pairing-info">
            <span className="channel-pairing-name">{copy.paired}</span>
            <span className="channel-pairing-meta">
              {pairing.openidMasked || copy.paired}
            </span>
          </span>
          <div className="channel-pairing-actions">
            {openHref ? (
              <a
                className="channel-pairing-open"
                href={openHref}
                target="_blank"
                rel="noreferrer"
              >
                {t.userModal.pairingOpenBot}
              </a>
            ) : null}
            <button
              type="button"
              className="channel-pairing-btn danger"
              disabled={busy}
              onClick={() => {
                if (!window.confirm(copy.unpairConfirm)) return
                void handleUnpair()
              }}
            >
              {copy.unpair}
            </button>
          </div>
        </div>
      ) : (
        <div className="channel-pairing-row">
          <span className="channel-pairing-info">
            <span
              className={`channel-pairing-name${liveCode ? ' channel-pairing-code' : ''}`}
            >
              {liveCode || copy.notPaired}
            </span>
            <span className="channel-pairing-meta">
              {liveCode ? copy.sendCode : copy.generateHint}
              {expiryLabel ? ` · ${expiryLabel}` : ''}
            </span>
          </span>
          <div className="channel-pairing-actions">
            {openHref ? (
              <a
                className="channel-pairing-open"
                href={openHref}
                target="_blank"
                rel="noreferrer"
              >
                {t.userModal.pairingOpenBot}
              </a>
            ) : null}
            {liveCode ? (
              <button
                type="button"
                className="channel-pairing-btn"
                onClick={() => void handleCopy(liveCode)}
              >
                <LuCopy size={14} aria-hidden />
                {copied ? t.common.copied : copy.copy}
              </button>
            ) : null}
            <button
              type="button"
              className="channel-pairing-btn"
              disabled={busy}
              onClick={() => void handleIssue()}
            >
              {liveCode ? copy.refresh : copy.generate}
            </button>
          </div>
        </div>
      )}
      {!receiveReady ? (
        <p className="channel-pairing-meta">{t.userModal.pairingWaitingReceive}</p>
      ) : null}
      {error ? <p className="channel-pairing-error">{error}</p> : null}
      <p className="channel-pairing-work">{t.config.channelWorkHint}</p>
    </section>
  )
}

function pairingCopy(
  t: ReturnType<typeof useI18n>['t'],
  channel: ChannelPairingKind,
) {
  if (channel === 'feishu') {
    return {
      title: t.userModal.feishuPairingTitle,
      hint: t.userModal.feishuPairingHint,
      loadFailed: t.userModal.feishuPairingLoadFailed,
      issueFailed: t.userModal.feishuPairingIssueFailed,
      copyFailed: t.userModal.feishuPairingCopyFailed,
      generateHint: t.userModal.feishuPairingGenerateHint,
      sendCode: t.userModal.feishuPairingSendCode,
      generate: t.userModal.feishuGenerateCode,
      refresh: t.userModal.feishuRefreshCode,
      copy: t.userModal.feishuCopyCode,
      paired: t.userModal.feishuPaired,
      notPaired: t.userModal.feishuNotPaired,
      unpair: t.userModal.feishuUnpair,
      unpairConfirm: t.userModal.feishuUnpairConfirm,
      unpairFailed: t.userModal.feishuUnpairFailed,
    }
  }
  if (channel === 'discord_dm') {
    return {
      title: t.userModal.discordPairingTitle,
      hint: t.userModal.discordPairingHint,
      loadFailed: t.userModal.discordPairingLoadFailed,
      issueFailed: t.userModal.discordPairingIssueFailed,
      copyFailed: t.userModal.discordPairingCopyFailed,
      generateHint: t.userModal.discordPairingGenerateHint,
      sendCode: t.userModal.discordPairingSendCode,
      generate: t.userModal.discordGenerateCode,
      refresh: t.userModal.discordRefreshCode,
      copy: t.userModal.discordCopyCode,
      paired: t.userModal.discordPaired,
      notPaired: t.userModal.discordNotPaired,
      unpair: t.userModal.discordUnpair,
      unpairConfirm: t.userModal.discordUnpairConfirm,
      unpairFailed: t.userModal.discordUnpairFailed,
    }
  }
  if (channel === 'qq') {
    return {
      title: t.userModal.qqPairingTitle,
      hint: t.userModal.qqPairingHint,
      loadFailed: t.userModal.qqPairingLoadFailed,
      issueFailed: t.userModal.qqPairingIssueFailed,
      copyFailed: t.userModal.qqPairingCopyFailed,
      generateHint: t.userModal.qqPairingGenerateHint,
      sendCode: t.userModal.qqPairingSendCode,
      generate: t.userModal.qqGenerateCode,
      refresh: t.userModal.qqRefreshCode,
      copy: t.userModal.qqCopyCode,
      paired: t.userModal.qqPaired,
      notPaired: t.userModal.qqNotPaired,
      unpair: t.userModal.qqUnpair,
      unpairConfirm: t.userModal.qqUnpairConfirm,
      unpairFailed: t.userModal.qqUnpairFailed,
    }
  }
  return {
    title: t.userModal.telegramPairingTitle,
    hint: t.userModal.telegramPairingHint,
    loadFailed: t.userModal.telegramPairingLoadFailed,
    issueFailed: t.userModal.telegramPairingIssueFailed,
    copyFailed: t.userModal.telegramPairingCopyFailed,
    generateHint: t.userModal.telegramPairingGenerateHint,
    sendCode: t.userModal.telegramPairingSendCode,
    generate: t.userModal.telegramGenerateCode,
    refresh: t.userModal.telegramRefreshCode,
    copy: t.userModal.telegramCopyCode,
    paired: t.userModal.telegramPaired,
    notPaired: t.userModal.telegramNotPaired,
    unpair: t.userModal.telegramUnpair,
    unpairConfirm: t.userModal.telegramUnpairConfirm,
    unpairFailed: t.userModal.telegramUnpairFailed,
  }
}
