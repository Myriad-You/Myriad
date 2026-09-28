export const AGENT_CHANNEL_IDS = [
  'qq',
  'telegram',
  'discord',
  'feishu',
  'onebot',
] as const

export type AgentChannelId = (typeof AGENT_CHANNEL_IDS)[number]

export const AGENT_CHANNEL_FIELDS: Record<
  AgentChannelId,
  { enabled: string; credentials: readonly string[] }
> = {
  qq: {
    enabled: 'qq_bot_enabled',
    credentials: ['qq_bot_app_id', 'qq_bot_app_secret'],
  },
  telegram: {
    enabled: 'telegram_bot_enabled',
    credentials: ['telegram_bot_token'],
  },
  discord: {
    enabled: 'discord_bot_enabled',
    credentials: ['discord_bot_token'],
  },
  feishu: {
    enabled: 'feishu_bot_enabled',
    credentials: ['feishu_bot_app_id', 'feishu_bot_app_secret'],
  },
  onebot: {
    enabled: 'onebot_bot_enabled',
    credentials: ['onebot_bot_ws_url', 'onebot_bot_access_token'],
  },
}

export function fieldHasStoredValue(value: string | undefined): boolean {
  return Boolean(value?.trim())
}

export function channelIsStored(
  id: AgentChannelId,
  getFieldValue: (key: string) => string,
): boolean {
  const fields = AGENT_CHANNEL_FIELDS[id]
  if (getFieldValue(fields.enabled) === 'true') return true
  return fields.credentials.some((key) => fieldHasStoredValue(getFieldValue(key)))
}

export function visibleAgentChannels(
  getFieldValue: (key: string) => string,
  revealed: Iterable<AgentChannelId>,
): AgentChannelId[] {
  const extra = new Set(revealed)
  return AGENT_CHANNEL_IDS.filter(
    (id) => channelIsStored(id, getFieldValue) || extra.has(id),
  )
}

export function clearAgentChannelValues(
  id: AgentChannelId,
  updateValue: (key: string, value: string) => void,
): void {
  const fields = AGENT_CHANNEL_FIELDS[id]
  updateValue(fields.enabled, 'false')
  for (const key of fields.credentials) updateValue(key, '')
}
