import type { PermissionConfigValues } from '../types'
import {
  fetchPermissionsConfig,
  updatePermissionsConfig,
} from '../../../../services/configApi'
import { DEFAULT_PERMISSION_CONFIG } from '../defaults'
import { useConfigDomain } from '../useConfigDomain'

export function usePermissionDomain(messages: {
  loadConfigFailed: string
  permissionsSaveFailed: string
}) {
  return useConfigDomain<PermissionConfigValues>({
    id: 'permissions',
    sections: ['permissions'],
    initial: DEFAULT_PERMISSION_CONFIG,
    load: async () => {
      const response = await fetchPermissionsConfig()
      if (!response.success || !response.config)
        throw new Error(messages.loadConfigFailed)
      const { guest, user, user_ai_quota, guest_ai_quota } = response.config
      const loaded: PermissionConfigValues = {
        user_perm_ai_generate: user.ai_generate,
        user_perm_ai_analyze: user.ai_analyze,
        user_perm_ai_chat: user.ai_chat,
        user_perm_report_write: user.report_write,
        user_perm_network_fetch: user.network_fetch,
        user_perm_component_theme: user.component_theme,
        user_perm_shortcut_register: user.shortcut_register,
        user_perm_event_publish: user.event_publish,
        user_perm_ai_image: user.ai_image,
        user_perm_ai_search: user.ai_search ?? false,
        user_perm_3d_generate: user.three_d_generate ?? false,
        user_perm_scheduler_register: user.scheduler_register,
        user_perm_speech_tts: user.speech_tts,
        user_perm_speech_asr: user.speech_asr,
        user_perm_storage_write: user.storage_write ?? false,
        user_perm_federation_post: user.federation_post ?? false,
        user_perm_federation_channel: user.federation_channel ?? false,
        user_perm_federation_room: user.federation_room ?? false,
        user_perm_phantasi_comment_write: user.phantasi_comment_write ?? false,
        guest_perm_ai_generate: guest.ai_generate,
        guest_perm_ai_analyze: guest.ai_analyze,
        guest_perm_ai_chat: guest.ai_chat,
        guest_perm_report_write: guest.report_write,
        guest_perm_network_fetch: guest.network_fetch,
        guest_perm_component_theme: guest.component_theme,
        guest_perm_shortcut_register: guest.shortcut_register,
        guest_perm_event_publish: guest.event_publish,
        guest_perm_ai_image: guest.ai_image,
        guest_perm_ai_search: guest.ai_search ?? false,
        guest_perm_3d_generate: guest.three_d_generate ?? false,
        guest_perm_scheduler_register: guest.scheduler_register,
        guest_perm_speech_tts: guest.speech_tts,
        guest_perm_speech_asr: guest.speech_asr,
        guest_perm_storage_write: guest.storage_write ?? false,
        guest_perm_federation_post: guest.federation_post ?? false,
        guest_perm_federation_channel: guest.federation_channel ?? false,
        guest_perm_federation_room: guest.federation_room ?? false,
        guest_perm_phantasi_comment_write: guest.phantasi_comment_write ?? false,
        user_ai_daily_calls: user_ai_quota?.daily_calls ?? 50,
        user_ai_daily_tokens: user_ai_quota?.daily_tokens ?? 20000,
        user_ai_cooldown_seconds: user_ai_quota?.cooldown_seconds ?? 5,
        guest_ai_daily_calls: guest_ai_quota?.daily_calls ?? 10,
        guest_ai_daily_tokens: guest_ai_quota?.daily_tokens ?? 5000,
        guest_ai_cooldown_seconds: guest_ai_quota?.cooldown_seconds ?? 10,
      }
      return loaded
    },
    persist: async (draft, saved) => {
      const patch = Object.fromEntries(
        Object.entries(draft).filter(([key, value]) => saved[key] !== value),
      )
      const response = await updatePermissionsConfig(patch)
      if (!response.success)
        throw new Error(response.message || messages.permissionsSaveFailed)
      return draft
    },
    reset: (_saved, scope) =>
      scope === 'permissions' || scope === 'all'
        ? structuredClone(DEFAULT_PERMISSION_CONFIG)
        : undefined,
    effects: () => [
      {
        id: 'grants',
        run: async () => {
          const { TappRuntime } =
            await import('../../../../tapp/runtime/TappRuntime')
          await TappRuntime.getInstance().refreshPermissionGrants()
        },
      },
    ],
  })
}
