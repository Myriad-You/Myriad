import type { ReactNode } from 'react'
import type {
  DiscordBotPhase,
  DiscordBotStatus,
  FeishuBotPhase,
  FeishuBotStatus,
  OneBotStatus,
  QqBotPhase,
  QqBotStatus,
  TelegramBotPhase,
  TelegramBotStatus,
} from '../../services/agent/agentApi'
import type { AgentChannelId } from './agentChannels'
import {
  FaPlus,
  FaTrash,
  LuMessageSquare,
  SiDiscord,
  SiQq,
  SiTelegram,
} from '@lib/icons'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useConfigI18n as useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'
import { showStickyToast, showToast } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
import {
  discordOpenHref,
  formatInboundTime,
  telegramOpenHref,
} from '../channel/channelPairing'
import { ChannelPairingPanel } from '../channel/ChannelPairingPanel'
import {
  CheckboxCard,
  CollapseRegion,
  guideDomProps,
  InputItem,
  SettingsButton,
  SettingTitleGuideEntry,
  SettingTitleTag,
  ToggleSwitch,
  useSettingGuide,
} from '../settings'
import {
  AGENT_CHANNEL_FIELDS,
  fieldHasStoredValue,
} from './agentChannels'
import { useAddedCardOpen } from './useAddedCard'
import './AiVendorAdd.css'

function notifyConfigAction(message: string, ok: boolean, replaceKey: string) {
  if (ok) {
    showToast({ message, type: 'success', replaceKey })
    return
  }
  showStickyToast({ message, type: 'error', replaceKey })
}

function ChannelIcon({ id }: { id: AgentChannelId }) {
  switch (id) {
    case 'qq':
      return <SiQq />
    case 'telegram':
      return <SiTelegram />
    case 'discord':
      return <SiDiscord />
    case 'feishu':
    case 'onebot':
      return <LuMessageSquare />
  }
}

function botPhaseLabel(
  phase:
    | QqBotPhase
    | TelegramBotPhase
    | DiscordBotPhase
    | FeishuBotPhase
    | undefined,
  labels: {
    online: string
    connecting: string
    reconnecting: string
    rejected: string
    offline: string
  },
) {
  switch (phase) {
    case 'online':
      return labels.online
    case 'connecting':
      return labels.connecting
    case 'reconnecting':
      return labels.reconnecting
    case 'rejected':
      return labels.rejected
    default:
      return labels.offline
  }
}

export function AgentChannelAddTrigger({
  visible,
  onAdd,
}: {
  visible: readonly AgentChannelId[]
  onAdd: (id: AgentChannelId) => void
}) {
  const { t } = useI18n()
  const added = useMemo(() => new Set(visible), [visible])
  const presets = useMemo(
    () =>
      [
        { id: 'qq' as const, title: t.config.qqBotTitle },
        { id: 'telegram' as const, title: t.config.telegramBotTitle },
        { id: 'discord' as const, title: t.config.discordBotTitle },
        { id: 'feishu' as const, title: t.config.feishuBotTitle },
        { id: 'onebot' as const, title: t.config.onebotBotTitle },
      ] satisfies Array<{ id: AgentChannelId; title: string }>,
    [t.config],
  )

  const guide = useMemo(
    () => (
      <div className="oidc-preset-grid">
        {presets.map((preset) => {
          const used = added.has(preset.id)
          return (
            <button
              key={preset.id}
              type="button"
              className={`oidc-preset-card${used ? ' is-used' : ''}`}
              onClick={() => {
                if (!used) onAdd(preset.id)
              }}
              title={[preset.title, used ? t.config.agentChannelAdded : '']
                .filter(Boolean)
                .join(' · ')}
            >
              <span className="oidc-preset-icon">
                <ChannelIcon id={preset.id} />
              </span>
              <span className="oidc-preset-name">{preset.title}</span>
              {used ? (
                <span className="ai-vendor-preset-used">
                  {t.config.agentChannelAdded}
                </span>
              ) : null}
            </button>
          )
        })}
      </div>
    ),
    [added, onAdd, presets, t.config.agentChannelAdded],
  )

  return (
    <SettingTitleGuideEntry
      title={t.config.agentChannelAdd}
      requireShowDetails={false}
      className="ai-vendor-add-entry"
      panelClassName="ai-vendor-add-float"
      guide={guide}
      renderTrigger={({ open, closing, toggle, ariaLabel }) => (
        <CheckboxCard
          variant="switch"
          label={t.config.agentChannelAdd}
          description={t.config.agentChannelAddDesc}
          icon={<FaPlus />}
          showIndicator={false}
          checked={open || closing}
          onChange={() => toggle()}
          title={t.config.agentChannelAddDesc}
          aria-label={ariaLabel}
          aria-expanded={open}
          className="ai-vendor-add-toggle settings-help-toggle"
        />
      )}
    />
  )
}

export function AgentChannelSources({
  visible,
  justAdded,
  getFieldValue,
  updateValue,
  onRemove,
}: {
  visible: readonly AgentChannelId[]
  justAdded: string | null
  getFieldValue: (key: string, defaultValue?: string) => string
  updateValue: (key: string, value: string) => void
  onRemove: (id: AgentChannelId) => void
}) {
  const { t } = useI18n()

  return (
    <div className="oidc-section">
      {visible.length === 0 ? (
        <div className="oidc-empty ai-vendor-empty">
          <span>{t.config.agentChannelEmpty}</span>
        </div>
      ) : null}
      {visible.map((id) => (
        <AgentChannelCard
          key={id}
          id={id}
          justAdded={id === justAdded}
          getFieldValue={getFieldValue}
          updateValue={updateValue}
          onRemove={() => onRemove(id)}
        />
      ))}
    </div>
  )
}

function AgentChannelCard({
  id,
  justAdded,
  getFieldValue,
  updateValue,
  onRemove,
}: {
  id: AgentChannelId
  justAdded: boolean
  getFieldValue: (key: string, defaultValue?: string) => string
  updateValue: (key: string, value: string) => void
  onRemove: () => void
}) {
  const { t } = useI18n()
  switch (id) {
    case 'qq':
      return (
        <QqChannelCard
          justAdded={justAdded}
          getFieldValue={getFieldValue}
          updateValue={updateValue}
          onRemove={onRemove}
          removeLabel={t.common.delete}
        />
      )
    case 'telegram':
      return (
        <TelegramChannelCard
          justAdded={justAdded}
          getFieldValue={getFieldValue}
          updateValue={updateValue}
          onRemove={onRemove}
          removeLabel={t.common.delete}
        />
      )
    case 'discord':
      return (
        <DiscordChannelCard
          justAdded={justAdded}
          getFieldValue={getFieldValue}
          updateValue={updateValue}
          onRemove={onRemove}
          removeLabel={t.common.delete}
        />
      )
    case 'feishu':
      return (
        <FeishuChannelCard
          justAdded={justAdded}
          getFieldValue={getFieldValue}
          updateValue={updateValue}
          onRemove={onRemove}
          removeLabel={t.common.delete}
        />
      )
    case 'onebot':
      return (
        <OneBotChannelCard
          justAdded={justAdded}
          getFieldValue={getFieldValue}
          updateValue={updateValue}
          onRemove={onRemove}
          removeLabel={t.common.delete}
        />
      )
  }
}

function ChannelRemoveButton({
  label,
  onRemove,
}: {
  label: string
  onRemove: () => void
}) {
  return (
    <SettingsButton
      variant="danger"
      size="sm"
      icon={<FaTrash />}
      onClick={onRemove}
      aria-label={label}
    />
  )
}

function ChannelCardFrame({
  id,
  title,
  justAdded,
  configured,
  enabled,
  onEnabledChange,
  enableLabel,
  enableTitle,
  badge,
  guide,
  guidePath,
  onRemove,
  removeLabel,
  children,
}: {
  id: AgentChannelId
  title: string
  justAdded: boolean
  configured: boolean
  enabled: boolean
  onEnabledChange: (value: boolean) => void
  enableLabel: string
  enableTitle?: string
  badge?: ReactNode
  guide?: ReactNode
  guidePath?: string
  onRemove: () => void
  removeLabel: string
  children: ReactNode
}) {
  const { t, format } = useI18n()
  const [open, setOpen] = useAddedCardOpen(justAdded, !configured)
  const toggleOpen = () => setOpen((value) => !value)

  return (
    <div
      className={`oidc-provider-card ai-vendor-card${open ? ' is-open' : ''}${
        enabled ? '' : ' disabled'
      }${justAdded ? ' is-added' : ''}${guidePath ? ' has-guide-anchor' : ''}`}
      {...guideDomProps(guidePath)}
    >
      <div className="oidc-provider-header ai-vendor-card-header">
        <button
          type="button"
          className="ai-vendor-card-hit"
          onClick={toggleOpen}
          aria-expanded={open}
          aria-label={format(
            open ? t.config.collapseGroupAria : t.config.expandGroupAria,
            { title },
          )}
        />
        <div className="oidc-provider-title">
          <span className="oidc-provider-icon-img" aria-hidden>
            <ChannelIcon id={id} />
          </span>
          <span className="oidc-provider-title-text">{title}</span>
          <span className="ai-vendor-card-tags">
            {badge}
            <span className="ai-vendor-card-control">
              <SettingTitleGuideEntry title={title} guide={guide} />
            </span>
            <SettingTitleTag variant="muted">
              {open ? t.config.aiVendorCollapse : t.config.aiVendorExpand}
            </SettingTitleTag>
          </span>
        </div>
        <div className="oidc-provider-actions">
          <div className="oidc-enable-toggle">
            <ToggleSwitch
              checked={enabled}
              onChange={onEnabledChange}
              aria-label={enableLabel}
              title={enableTitle}
            />
          </div>
          <ChannelRemoveButton label={removeLabel} onRemove={onRemove} />
        </div>
      </div>
      <CollapseRegion open={open}>
        <div className="ai-vendor-card-body">{children}</div>
      </CollapseRegion>
    </div>
  )
}

function channelCredentialsFilled(
  id: AgentChannelId,
  getFieldValue: (key: string, defaultValue?: string) => string,
) {
  return AGENT_CHANNEL_FIELDS[id].credentials.some((key) =>
    fieldHasStoredValue(getFieldValue(key)),
  )
}

function QqChannelCard({
  justAdded,
  getFieldValue,
  updateValue,
  onRemove,
  removeLabel,
}: {
  justAdded: boolean
  getFieldValue: (key: string, defaultValue?: string) => string
  updateValue: (key: string, value: string) => void
  onRemove: () => void
  removeLabel: string
}) {
  const { t, locale, format } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const [status, setStatus] = useState<QqBotStatus | null>(null)
  const [testing, setTesting] = useState(false)
  const [testMessage, setTestMessage] = useState<string | null>(null)

  const loadStatus = useCallback(async () => {
    try {
      setStatus(await agentService.getQqBotStatus())
    } catch {
      setStatus(null)
    }
  }, [])

  useEffect(() => {
    void loadStatus()
    const timer = window.setInterval(() => {
      void loadStatus()
    }, 3000)
    return () => window.clearInterval(timer)
  }, [loadStatus])

  const handleTest = useCallback(async () => {
    setTesting(true)
    setTestMessage(null)
    try {
      await agentService.testQqBot()
      setTestMessage(t.config.qqBotTestOk)
      notifyConfigAction(t.config.qqBotTestOk, true, 'config-bot-test')
      await loadStatus()
    } catch (error) {
      const message = userFacingError(error, t.config.qqBotTestFailed)
      setTestMessage(message)
      notifyConfigAction(message, false, 'config-bot-test')
    } finally {
      setTesting(false)
    }
  }, [loadStatus, t.config.qqBotTestFailed, t.config.qqBotTestOk])

  const phase = botPhaseLabel(status?.phase, {
    online: t.config.qqBotPhaseOnline,
    connecting: t.config.qqBotPhaseConnecting,
    reconnecting: t.config.qqBotPhaseReconnecting,
    rejected: t.config.qqBotPhaseRejected,
    offline: t.config.qqBotPhaseOffline,
  })

  return (
    <ChannelCardFrame
      id="qq"
      title={t.config.qqBotTitle}
      justAdded={justAdded}
      configured={channelCredentialsFilled('qq', getFieldValue)}
      enabled={getFieldValue('qq_bot_enabled') === 'true'}
      onEnabledChange={(value) =>
        updateValue('qq_bot_enabled', value ? 'true' : 'false')
      }
      enableLabel={t.config.qqBotTitle}
      enableTitle={t.config.qqBotHint}
      {...bindGuide('agent.qqBot', g.agent.qqBot)}
      badge={
        <SettingTitleTag
          variant={status?.phase === 'rejected' ? 'danger' : 'muted'}
          title={testMessage || t.config.qqBotHint}
        >
          {phase}
        </SettingTitleTag>
      }
      onRemove={onRemove}
      removeLabel={removeLabel}
    >
      <InputItem
        itemKey="qq_bot_app_id"
        label={t.config.qqBotAppId}
        value={getFieldValue('qq_bot_app_id')}
        onChange={(value) => updateValue('qq_bot_app_id', value)}
        placeholder="102..."
        hint={t.config.qqBotHint}
        layout="vertical"
        {...bindGuide('agent.qqBot', g.agent.qqBot)}
      />
      <InputItem
        itemKey="qq_bot_app_secret"
        label={t.config.qqBotAppSecret}
        value={getFieldValue('qq_bot_app_secret')}
        onChange={(value) => updateValue('qq_bot_app_secret', value)}
        inputType="password"
        autoSelectOnMask
        layout="vertical"
        {...bindGuide('agent.qqBot', g.agent.qqBot)}
      />
      <SettingsButton
        size="sm"
        loading={testing}
        disabled={testing}
        onClick={() => void handleTest()}
      >
        {testing ? t.config.qqBotTesting : t.config.qqBotTest}
      </SettingsButton>
      <ChannelConnectFacts
        credentialLabel={t.config.qqBotCredentialLabel}
        credentialValue={testMessage || t.config.qqBotCredentialUntested}
        receiveLabel={t.config.qqBotReceiveLabel}
        receiveValue={phase}
        identityLabel={t.config.qqBotIdentityLabel}
        identityValue={status?.appId ? `AppID ${status.appId}` : null}
        inboundLabel={
          formatInboundTime(status?.lastInboundAt, locale)
            ? format(t.config.qqBotLastInbound, {
                time: formatInboundTime(
                  status?.lastInboundAt,
                  locale,
                ) as string,
              })
            : t.config.qqBotLastInboundNone
        }
        openHint={t.config.qqBotOpenHint}
      />
      {getFieldValue('qq_bot_enabled') === 'true' &&
      (status?.hasAppId || status?.hasSecret) ? (
        <ChannelPairingPanel
          channel="qq"
          receiveReady={status?.phase === 'online'}
        />
      ) : null}
    </ChannelCardFrame>
  )
}

function TelegramChannelCard({
  justAdded,
  getFieldValue,
  updateValue,
  onRemove,
  removeLabel,
}: {
  justAdded: boolean
  getFieldValue: (key: string, defaultValue?: string) => string
  updateValue: (key: string, value: string) => void
  onRemove: () => void
  removeLabel: string
}) {
  const { t, locale, format } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const [status, setStatus] = useState<TelegramBotStatus | null>(null)
  const [testing, setTesting] = useState(false)
  const [testMessage, setTestMessage] = useState<string | null>(null)

  const loadStatus = useCallback(async () => {
    try {
      setStatus(await agentService.getTelegramBotStatus())
    } catch {
      setStatus(null)
    }
  }, [])

  useEffect(() => {
    void loadStatus()
    const timer = window.setInterval(() => {
      void loadStatus()
    }, 3000)
    return () => window.clearInterval(timer)
  }, [loadStatus])

  const handleTest = useCallback(async () => {
    setTesting(true)
    setTestMessage(null)
    try {
      const tested = await agentService.testTelegramBot()
      setTestMessage(t.config.telegramBotTestOk)
      notifyConfigAction(t.config.telegramBotTestOk, true, 'config-bot-test')
      if (tested.botUsername || tested.botName) {
        setStatus((current) =>
          current
            ? {
                ...current,
                botUsername: tested.botUsername ?? current.botUsername,
                botName: tested.botName ?? current.botName,
              }
            : current,
        )
      }
      await loadStatus()
    } catch (error) {
      const message = userFacingError(error, t.config.telegramBotTestFailed)
      setTestMessage(message)
      notifyConfigAction(message, false, 'config-bot-test')
    } finally {
      setTesting(false)
    }
  }, [
    loadStatus,
    t.config.telegramBotTestFailed,
    t.config.telegramBotTestOk,
  ])

  const phase = botPhaseLabel(status?.phase, {
    online: t.config.telegramBotPhaseOnline,
    connecting: t.config.telegramBotPhaseConnecting,
    reconnecting: t.config.telegramBotPhaseReconnecting,
    rejected: t.config.telegramBotPhaseRejected,
    offline: t.config.telegramBotPhaseOffline,
  })

  return (
    <ChannelCardFrame
      id="telegram"
      title={t.config.telegramBotTitle}
      justAdded={justAdded}
      configured={channelCredentialsFilled('telegram', getFieldValue)}
      enabled={getFieldValue('telegram_bot_enabled') === 'true'}
      onEnabledChange={(value) =>
        updateValue('telegram_bot_enabled', value ? 'true' : 'false')
      }
      enableLabel={t.config.telegramBotTitle}
      enableTitle={t.config.telegramBotHint}
      {...bindGuide('agent.telegramBot', g.agent.telegramBot)}
      badge={
        <SettingTitleTag
          variant={status?.phase === 'rejected' ? 'danger' : 'muted'}
          title={testMessage || t.config.telegramBotHint}
        >
          {phase}
        </SettingTitleTag>
      }
      onRemove={onRemove}
      removeLabel={removeLabel}
    >
      <InputItem
        itemKey="telegram_bot_token"
        label={t.config.telegramBotToken}
        value={getFieldValue('telegram_bot_token')}
        onChange={(value) => updateValue('telegram_bot_token', value)}
        inputType="password"
        autoSelectOnMask
        hint={t.config.telegramBotHint}
        layout="vertical"
        {...bindGuide('agent.telegramBot', g.agent.telegramBot)}
      />
      <SettingsButton
        size="sm"
        loading={testing}
        disabled={testing}
        onClick={() => void handleTest()}
      >
        {testing ? t.config.telegramBotTesting : t.config.telegramBotTest}
      </SettingsButton>
      <ChannelConnectFacts
        credentialLabel={t.config.telegramBotCredentialLabel}
        credentialValue={
          testMessage || t.config.telegramBotCredentialUntested
        }
        receiveLabel={t.config.telegramBotReceiveLabel}
        receiveValue={phase}
        identityLabel={t.config.telegramBotIdentityLabel}
        identityValue={
          status?.botUsername
            ? format(t.config.telegramBotIdentityName, {
                name: status.botName || status.botUsername,
                username: status.botUsername,
              })
            : status?.botName
              ? format(t.config.telegramBotIdentityNameOnly, {
                  name: status.botName,
                })
              : null
        }
        inboundLabel={
          formatInboundTime(status?.lastInboundAt, locale)
            ? format(t.config.telegramBotLastInbound, {
                time: formatInboundTime(
                  status?.lastInboundAt,
                  locale,
                ) as string,
              })
            : t.config.telegramBotLastInboundNone
        }
        openHref={telegramOpenHref(status?.botUsername)}
        openLabel={t.config.telegramBotOpen}
      />
      {getFieldValue('telegram_bot_enabled') === 'true' && status?.hasToken ? (
        <ChannelPairingPanel
          channel="telegram"
          openHref={telegramOpenHref(status?.botUsername)}
          receiveReady={status?.phase === 'online'}
        />
      ) : null}
    </ChannelCardFrame>
  )
}

function DiscordChannelCard({
  justAdded,
  getFieldValue,
  updateValue,
  onRemove,
  removeLabel,
}: {
  justAdded: boolean
  getFieldValue: (key: string, defaultValue?: string) => string
  updateValue: (key: string, value: string) => void
  onRemove: () => void
  removeLabel: string
}) {
  const { t, locale, format } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const [status, setStatus] = useState<DiscordBotStatus | null>(null)
  const [testing, setTesting] = useState(false)
  const [testMessage, setTestMessage] = useState<string | null>(null)

  const loadStatus = useCallback(async () => {
    try {
      setStatus(await agentService.getDiscordBotStatus())
    } catch {
      setStatus(null)
    }
  }, [])

  useEffect(() => {
    void loadStatus()
    const timer = window.setInterval(() => {
      void loadStatus()
    }, 3000)
    return () => window.clearInterval(timer)
  }, [loadStatus])

  const handleTest = useCallback(async () => {
    setTesting(true)
    setTestMessage(null)
    try {
      const tested = await agentService.testDiscordBot()
      setTestMessage(t.config.discordBotTestOk)
      notifyConfigAction(t.config.discordBotTestOk, true, 'config-bot-test')
      if (tested.botUsername || tested.botName || tested.botUserId) {
        setStatus((current) =>
          current
            ? {
                ...current,
                botUsername: tested.botUsername ?? current.botUsername,
                botName: tested.botName ?? current.botName,
                botUserId: tested.botUserId ?? current.botUserId,
              }
            : current,
        )
      }
      await loadStatus()
    } catch (error) {
      const message = userFacingError(error, t.config.discordBotTestFailed)
      setTestMessage(message)
      notifyConfigAction(message, false, 'config-bot-test')
    } finally {
      setTesting(false)
    }
  }, [loadStatus, t.config.discordBotTestFailed, t.config.discordBotTestOk])

  const phase = botPhaseLabel(status?.phase, {
    online: t.config.discordBotPhaseOnline,
    connecting: t.config.discordBotPhaseConnecting,
    reconnecting: t.config.discordBotPhaseReconnecting,
    rejected: t.config.discordBotPhaseRejected,
    offline: t.config.discordBotPhaseOffline,
  })

  return (
    <ChannelCardFrame
      id="discord"
      title={t.config.discordBotTitle}
      justAdded={justAdded}
      configured={channelCredentialsFilled('discord', getFieldValue)}
      enabled={getFieldValue('discord_bot_enabled') === 'true'}
      onEnabledChange={(value) =>
        updateValue('discord_bot_enabled', value ? 'true' : 'false')
      }
      enableLabel={t.config.discordBotTitle}
      enableTitle={t.config.discordBotHint}
      {...bindGuide('agent.discordBot', g.agent.discordBot)}
      badge={
        <SettingTitleTag
          variant={status?.phase === 'rejected' ? 'danger' : 'muted'}
          title={testMessage || t.config.discordBotHint}
        >
          {phase}
        </SettingTitleTag>
      }
      onRemove={onRemove}
      removeLabel={removeLabel}
    >
      <InputItem
        itemKey="discord_bot_token"
        label={t.config.discordBotToken}
        value={getFieldValue('discord_bot_token')}
        onChange={(value) => updateValue('discord_bot_token', value)}
        inputType="password"
        autoSelectOnMask
        hint={t.config.discordBotHint}
        layout="vertical"
        {...bindGuide('agent.discordBot', g.agent.discordBot)}
      />
      <SettingsButton
        size="sm"
        loading={testing}
        disabled={testing}
        onClick={() => void handleTest()}
      >
        {testing ? t.config.discordBotTesting : t.config.discordBotTest}
      </SettingsButton>
      <ChannelConnectFacts
        credentialLabel={t.config.discordBotCredentialLabel}
        credentialValue={
          testMessage || t.config.discordBotCredentialUntested
        }
        receiveLabel={t.config.discordBotReceiveLabel}
        receiveValue={phase}
        identityLabel={t.config.discordBotIdentityLabel}
        identityValue={
          status?.botUsername
            ? format(t.config.discordBotIdentityName, {
                name: status.botName || status.botUsername,
                username: status.botUsername,
              })
            : status?.botName
              ? format(t.config.discordBotIdentityNameOnly, {
                  name: status.botName,
                })
              : null
        }
        inboundLabel={
          formatInboundTime(status?.lastInboundAt, locale)
            ? format(t.config.discordBotLastInbound, {
                time: formatInboundTime(
                  status?.lastInboundAt,
                  locale,
                ) as string,
              })
            : t.config.discordBotLastInboundNone
        }
        openHref={discordOpenHref(status?.botUserId)}
        openLabel={t.config.discordBotOpen}
      />
      {getFieldValue('discord_bot_enabled') === 'true' && status?.hasToken ? (
        <ChannelPairingPanel
          channel="discord_dm"
          openHref={discordOpenHref(status?.botUserId)}
          receiveReady={status?.phase === 'online'}
        />
      ) : null}
    </ChannelCardFrame>
  )
}

function FeishuChannelCard({
  justAdded,
  getFieldValue,
  updateValue,
  onRemove,
  removeLabel,
}: {
  justAdded: boolean
  getFieldValue: (key: string, defaultValue?: string) => string
  updateValue: (key: string, value: string) => void
  onRemove: () => void
  removeLabel: string
}) {
  const { t, locale, format } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const [status, setStatus] = useState<FeishuBotStatus | null>(null)
  const [testing, setTesting] = useState(false)
  const [testMessage, setTestMessage] = useState<string | null>(null)

  const loadStatus = useCallback(async () => {
    try {
      setStatus(await agentService.getFeishuBotStatus())
    } catch {
      setStatus(null)
    }
  }, [])

  useEffect(() => {
    void loadStatus()
    const timer = window.setInterval(() => {
      void loadStatus()
    }, 3000)
    return () => window.clearInterval(timer)
  }, [loadStatus])

  const handleTest = useCallback(async () => {
    setTesting(true)
    setTestMessage(null)
    try {
      await agentService.testFeishuBot()
      setTestMessage(t.config.feishuBotTestOk)
      notifyConfigAction(t.config.feishuBotTestOk, true, 'config-bot-test')
      await loadStatus()
    } catch (error) {
      const message = userFacingError(error, t.config.feishuBotTestFailed)
      setTestMessage(message)
      notifyConfigAction(message, false, 'config-bot-test')
    } finally {
      setTesting(false)
    }
  }, [loadStatus, t.config.feishuBotTestFailed, t.config.feishuBotTestOk])

  const phase = botPhaseLabel(status?.phase, {
    online: t.config.feishuBotPhaseOnline,
    connecting: t.config.feishuBotPhaseConnecting,
    reconnecting: t.config.feishuBotPhaseReconnecting,
    rejected: t.config.feishuBotPhaseRejected,
    offline: t.config.feishuBotPhaseOffline,
  })

  return (
    <ChannelCardFrame
      id="feishu"
      title={t.config.feishuBotTitle}
      justAdded={justAdded}
      configured={channelCredentialsFilled('feishu', getFieldValue)}
      enabled={getFieldValue('feishu_bot_enabled') === 'true'}
      onEnabledChange={(value) =>
        updateValue('feishu_bot_enabled', value ? 'true' : 'false')
      }
      enableLabel={t.config.feishuBotTitle}
      enableTitle={t.config.feishuBotHint}
      {...bindGuide('agent.feishuBot', g.agent.feishuBot)}
      badge={
        <SettingTitleTag
          variant={status?.phase === 'rejected' ? 'danger' : 'muted'}
          title={testMessage || t.config.feishuBotHint}
        >
          {phase}
        </SettingTitleTag>
      }
      onRemove={onRemove}
      removeLabel={removeLabel}
    >
      <InputItem
        itemKey="feishu_bot_app_id"
        label={t.config.feishuBotAppId}
        value={getFieldValue('feishu_bot_app_id')}
        onChange={(value) => updateValue('feishu_bot_app_id', value)}
        placeholder="cli_..."
        hint={t.config.feishuBotHint}
        layout="vertical"
        {...bindGuide('agent.feishuBot', g.agent.feishuBot)}
      />
      <InputItem
        itemKey="feishu_bot_app_secret"
        label={t.config.feishuBotAppSecret}
        value={getFieldValue('feishu_bot_app_secret')}
        onChange={(value) => updateValue('feishu_bot_app_secret', value)}
        inputType="password"
        autoSelectOnMask
        layout="vertical"
        {...bindGuide('agent.feishuBot', g.agent.feishuBot)}
      />
      <SettingsButton
        size="sm"
        loading={testing}
        disabled={testing}
        onClick={() => void handleTest()}
      >
        {testing ? t.config.feishuBotTesting : t.config.feishuBotTest}
      </SettingsButton>
      <ChannelConnectFacts
        credentialLabel={t.config.feishuBotCredentialLabel}
        credentialValue={testMessage || t.config.feishuBotCredentialUntested}
        receiveLabel={t.config.feishuBotReceiveLabel}
        receiveValue={phase}
        identityLabel={t.config.feishuBotIdentityLabel}
        identityValue={status?.appId ? `AppID ${status.appId}` : null}
        inboundLabel={
          formatInboundTime(status?.lastInboundAt, locale)
            ? format(t.config.feishuBotLastInbound, {
                time: formatInboundTime(
                  status?.lastInboundAt,
                  locale,
                ) as string,
              })
            : t.config.feishuBotLastInboundNone
        }
        openHint={t.config.feishuBotOpenHint}
      />
      {getFieldValue('feishu_bot_enabled') === 'true' &&
      (status?.hasAppId || status?.hasSecret) ? (
        <ChannelPairingPanel
          channel="feishu"
          receiveReady={status?.phase === 'online'}
        />
      ) : null}
    </ChannelCardFrame>
  )
}

function OneBotChannelCard({
  justAdded,
  getFieldValue,
  updateValue,
  onRemove,
  removeLabel,
}: {
  justAdded: boolean
  getFieldValue: (key: string, defaultValue?: string) => string
  updateValue: (key: string, value: string) => void
  onRemove: () => void
  removeLabel: string
}) {
  const { t, locale, format } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const [status, setStatus] = useState<OneBotStatus | null>(null)

  const loadStatus = useCallback(async () => {
    try {
      setStatus(await agentService.getOneBotStatus())
    } catch {
      setStatus(null)
    }
  }, [])

  useEffect(() => {
    void loadStatus()
    const timer = window.setInterval(() => {
      void loadStatus()
    }, 3000)
    return () => window.clearInterval(timer)
  }, [loadStatus])

  const phase = botPhaseLabel(status?.phase, {
    online: t.config.onebotBotPhaseOnline,
    connecting: t.config.onebotBotPhaseConnecting,
    reconnecting: t.config.onebotBotPhaseReconnecting,
    rejected: t.config.onebotBotPhaseRejected,
    offline: t.config.onebotBotPhaseOffline,
  })

  return (
    <ChannelCardFrame
      id="onebot"
      title={t.config.onebotBotTitle}
      justAdded={justAdded}
      configured={channelCredentialsFilled('onebot', getFieldValue)}
      enabled={getFieldValue('onebot_bot_enabled') === 'true'}
      onEnabledChange={(value) =>
        updateValue('onebot_bot_enabled', value ? 'true' : 'false')
      }
      enableLabel={t.config.onebotBotTitle}
      enableTitle={t.config.onebotBotHint}
      {...bindGuide('agent.onebotBot', g.agent.onebotBot)}
      badge={
        <SettingTitleTag
          variant={status?.phase === 'rejected' ? 'danger' : 'muted'}
          title={t.config.onebotBotHint}
        >
          {phase}
        </SettingTitleTag>
      }
      onRemove={onRemove}
      removeLabel={removeLabel}
    >
      <InputItem
        itemKey="onebot_bot_ws_url"
        label={t.config.onebotBotWsUrl}
        value={getFieldValue('onebot_bot_ws_url')}
        onChange={(value) => updateValue('onebot_bot_ws_url', value)}
        placeholder="ws://127.0.0.1:3001"
        hint={t.config.onebotBotHint}
        layout="vertical"
        {...bindGuide('agent.onebotBot', g.agent.onebotBot)}
      />
      <CheckboxCard
        label={t.config.onebotBotGroups}
        checked={getFieldValue('onebot_bot_groups_enabled') === 'true'}
        onChange={(value) =>
          updateValue('onebot_bot_groups_enabled', value ? 'true' : 'false')
        }
        hint={t.config.onebotBotGroupsHint}
      />
      <InputItem
        itemKey="onebot_bot_access_token"
        label={t.config.onebotBotAccessToken}
        value={getFieldValue('onebot_bot_access_token')}
        onChange={(value) => updateValue('onebot_bot_access_token', value)}
        inputType="password"
        autoSelectOnMask
        layout="vertical"
        {...bindGuide('agent.onebotBot', g.agent.onebotBot)}
      />
      <ChannelConnectFacts
        credentialLabel={t.config.onebotBotCredentialLabel}
        credentialValue={t.config.onebotBotCredentialUntested}
        receiveLabel={t.config.onebotBotReceiveLabel}
        receiveValue={phase}
        identityLabel={t.config.onebotBotIdentityLabel}
        identityValue={status?.wsUrl || null}
        inboundLabel={
          formatInboundTime(status?.lastInboundAt, locale)
            ? format(t.config.onebotBotLastInbound, {
                time: formatInboundTime(
                  status?.lastInboundAt,
                  locale,
                ) as string,
              })
            : t.config.onebotBotLastInboundNone
        }
        openHint={t.config.onebotBotOpenHint}
      />
      {getFieldValue('onebot_bot_enabled') === 'true' &&
      (status?.hasUrl || status?.hasToken) ? (
        <ChannelPairingPanel
          channel="onebot"
          receiveReady={status?.phase === 'online'}
        />
      ) : null}
    </ChannelCardFrame>
  )
}

function ChannelConnectFacts({
  credentialLabel,
  credentialValue,
  receiveLabel,
  receiveValue,
  identityLabel,
  identityValue,
  inboundLabel,
  openHref,
  openLabel,
  openHint,
}: {
  credentialLabel: string
  credentialValue: string
  receiveLabel: string
  receiveValue: string
  identityLabel: string
  identityValue: string | null
  inboundLabel: string
  openHref?: string | null
  openLabel?: string
  openHint?: string
}) {
  return (
    <div className="channel-connect-facts">
      <p className="channel-connect-fact">
        {credentialLabel}：{credentialValue}
      </p>
      <p className="channel-connect-fact">
        {receiveLabel}：{receiveValue} · {inboundLabel}
      </p>
      {identityValue ? (
        <p className="channel-connect-fact">
          {identityLabel}：{identityValue}
        </p>
      ) : null}
      {openHref && openLabel ? (
        <p className="channel-connect-fact">
          <a href={openHref} target="_blank" rel="noreferrer">
            {openLabel}
          </a>
        </p>
      ) : openHint ? (
        <p className="channel-connect-fact">{openHint}</p>
      ) : null}
    </div>
  )
}
