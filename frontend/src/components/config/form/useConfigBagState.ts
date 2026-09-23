import type { Config, PlatformAutoFetchConfig } from './types'
import { fetchConfig, updateConfig } from '../../../services/configApi'
import {
  hasBangumiCredential,
  isBangumiPlatform,
  sanitizeMaskedFieldValue,
} from '../platformConfigRules'
import { omitAgentOwnedFields } from './agentBagSlice'
import { configBagEffects } from './configBagEffects'
import { resetConfigBag } from './configBagReset'
import { DEFAULT_AUTO_FETCH_CONFIG } from './defaults'
import { useConfigDomain } from './useConfigDomain'

export function useConfigBagState(messages: {
  configEmpty: string
  configSaveFailed: string
  bangumiCredentialMissing: string
}) {
  const domain = useConfigDomain<Config | null>({
    id: 'config',
    initial: null,
    readBack: true,
    load: async () => {
      const data: Config = await fetchConfig()
      return omitAgentOwnedFields({
        ...data,
        auto_fetch: data.auto_fetch || DEFAULT_AUTO_FETCH_CONFIG,
      })
    },
    persist: async (config) => {
      if (!config) throw new Error(messages.configEmpty)
      if (
        config.platforms.some(
          (platform) =>
            platform.enabled &&
            isBangumiPlatform(platform) &&
            !hasBangumiCredential(platform),
        )
      ) {
        throw new Error(messages.bangumiCredentialMissing)
      }
      const result = await updateConfig(omitAgentOwnedFields(config))
      if (result?.success === false)
        throw new Error(result.message || messages.configSaveFailed)
      return omitAgentOwnedFields(config)
    },
    reset: resetConfigBag,
    effects: configBagEffects,
  })
  const updateConfigField = (
    section: 'ai_config' | 'tripo_config' | 'ui_config',
    fieldKey: string,
    value: string,
    silent = false,
  ) => {
    const patch = (config: Config | null): Config | null => {
      if (
        !config ||
        !config[section].config_fields.some((field) => field.key === fieldKey)
      ) {
        return config
      }
      return {
        ...config,
        [section]: {
          ...config[section],
          config_fields: config[section].config_fields.map((field) =>
            field.key === fieldKey
              ? { ...field, value: sanitizeMaskedFieldValue(value) }
              : field,
          ),
        },
      }
    }
    if (silent) domain.acceptPatch(patch)
    else domain.setDraft(patch)
  }
  return {
    ...domain,
    config: domain.draft,
    updateFieldValue: (index: number, key: string, value: string) =>
      domain.setDraft(
        (config) =>
          config && {
            ...config,
            platforms: config.platforms.map((platform, i) =>
              i !== index
                ? platform
                : {
                    ...platform,
                    config_fields: platform.config_fields.map((field) =>
                      field.key === key
                        ? { ...field, value: sanitizeMaskedFieldValue(value) }
                        : field,
                    ),
                  },
            ),
          },
      ),
    updateAiFieldValue: (key: string, value: string) =>
      updateConfigField('ai_config', key, value),
    updateTripoFieldValue: (key: string, value: string) =>
      updateConfigField('tripo_config', key, value),
    updateUiFieldValue: (
      key: string,
      value: string,
      options?: { silent?: boolean },
    ) => updateConfigField('ui_config', key, value, options?.silent),
    togglePlatform: (index: number) =>
      domain.setDraft(
        (config) =>
          config && {
            ...config,
            platforms: config.platforms.map((platform, i) =>
              i === index
                ? { ...platform, enabled: !platform.enabled }
                : platform,
            ),
          },
      ),
    updateAutoFetchConfig: (auto_fetch: PlatformAutoFetchConfig) =>
      domain.setDraft((config) => config && { ...config, auto_fetch }),
    reorderPlatform: (from: number, to: number) =>
      domain.setDraft((config) => {
        if (
          !config ||
          from === to ||
          from < 0 ||
          to < 0 ||
          from >= config.platforms.length ||
          to >= config.platforms.length
        ) {
          return config
        }
        return {
          ...config,
          platforms: config.platforms
            .toSpliced(from, 1)
            .toSpliced(to, 0, config.platforms[from]),
        }
      }),
  }
}
