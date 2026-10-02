import type { Config, ConfigField } from './types'
import { AGENT_UI_RESET_KEYS } from '../uiBagOwnership'
import { AGENT_AI_FIELD_KEYS, defaultAiFieldValue, defaultUiFieldValue, mapConfigFields } from './defaultFieldValues'
import { DEFAULT_AUTO_FETCH_CONFIG } from './defaults'

/** 人设开关要读，但不归 Agent 域写。 */
export const AGENT_GATE_AI_KEYS = new Set(['lite_ai_model', 'pro_enabled'])

export interface AgentSettingsSlice {
  aiFields: ConfigField[]
  uiFields: ConfigField[]
}

export const EMPTY_AGENT_SETTINGS_SLICE: AgentSettingsSlice = {
  aiFields: [],
  uiFields: [],
}

export function isAgentOwnedAiKey(key: string): boolean {
  return AGENT_AI_FIELD_KEYS.has(key)
}

export function isAgentOwnedUiKey(key: string): boolean {
  return (AGENT_UI_RESET_KEYS as readonly string[]).includes(key)
}

export function omitAgentOwnedFields(config: Config): Config {
  return {
    ...config,
    ai_config: {
      config_fields: config.ai_config.config_fields.filter(
        (field) => !isAgentOwnedAiKey(field.key),
      ),
    },
    ui_config: {
      config_fields: config.ui_config.config_fields.filter(
        (field) => !isAgentOwnedUiKey(field.key),
      ),
    },
  }
}

export function pickAgentSlice(config: Config): AgentSettingsSlice {
  return {
    aiFields: config.ai_config.config_fields.filter(
      (field) =>
        isAgentOwnedAiKey(field.key) || AGENT_GATE_AI_KEYS.has(field.key),
    ),
    uiFields: config.ui_config.config_fields.filter((field) =>
      isAgentOwnedUiKey(field.key),
    ),
  }
}

export function resetAgentSlice(slice: AgentSettingsSlice): AgentSettingsSlice {
  return {
    aiFields: mapConfigFields(
      slice.aiFields,
      defaultAiFieldValue,
      AGENT_AI_FIELD_KEYS,
    ),
    uiFields: mapConfigFields(
      slice.uiFields,
      defaultUiFieldValue,
      new Set(AGENT_UI_RESET_KEYS),
    ),
  }
}

/** POST /api/config 只带 Agent 自己的 key；缺字段后端不改。 */
export function agentSlicePersistPayload(slice: AgentSettingsSlice) {
  return {
    platforms: [],
    ai_config: {
      config_fields: slice.aiFields.filter((field) =>
        isAgentOwnedAiKey(field.key),
      ),
    },
    tripo_config: { config_fields: [] },
    ui_config: {
      config_fields: slice.uiFields.filter((field) =>
        isAgentOwnedUiKey(field.key),
      ),
    },
  }
}

export function agentSliceAsConfig(slice: AgentSettingsSlice): Config {
  return {
    platforms: [],
    auto_fetch: DEFAULT_AUTO_FETCH_CONFIG,
    ai_config: { config_fields: slice.aiFields },
    tripo_config: { config_fields: [] },
    ui_config: { config_fields: slice.uiFields },
  }
}
