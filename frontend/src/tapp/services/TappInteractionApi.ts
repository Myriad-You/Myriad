import type {
  AgentInteractionV2,
  PublishEventRequest,
  TappEvent,
} from '../types'
import { apiRequest, streamRuntimeEvents } from './TappHttpClient'

// ============ P2: Component Registration API ============

/** 组件类型 */
export type ComponentType = 'theme' | 'agent'

/** 组件配置基础接口 */
export interface ComponentConfig {
  id: string
  [key: string]: unknown
}

/** Theme 组件配置 */
export interface ThemeComponentConfig extends ComponentConfig {
  name: string
  /**
   * 小组件表面样式（受约束枚举）：'glass' | 'solid' | 'flat' | 'outline'
   * 宿主仅消费此白名单值，见 useTappThemes 的校验。
   */
  surface?: string
  /**
   * 小组件光晕模式（受约束枚举）：'identity' | 'primary' | 'none'
   */
  glow?: string
}

/** Agent 组件配置 */
export interface AgentComponentConfig extends ComponentConfig {
  name: string
  description?: string
  capabilities: string[]
}

/** 已注册组件 */
export interface RegisteredComponent {
  id: string
  type: ComponentType
  tappId: string
  config: ComponentConfig
  registeredAt: string
  enabled: boolean
}

/**
 * 注册组件
 */
export async function registerComponent(
  tappId: string,
  componentType: ComponentType,
  config: ComponentConfig,
  runtimeGrant?: string,
): Promise<{ success: boolean; component: RegisteredComponent }> {
  return apiRequest('/api/tapp/components/register', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: tappId,
      component_type: componentType,
      config,
    }),
    runtimeGrant,
  })
}

/**
 * 注销组件
 */
export async function unregisterComponent(
  tappId: string,
  componentType: ComponentType,
  componentId: string,
  runtimeGrant?: string,
): Promise<{
  success: boolean
  unregistered: { id: string; type: string; tappId: string }
}> {
  return apiRequest(
    `/api/tapp/components/${encodeURIComponent(tappId)}/${componentType}/${encodeURIComponent(componentId)}`,
    {
      method: 'DELETE',
      runtimeGrant,
    },
  )
}

/**
 * 列出 Tapp 的已注册组件
 */
export async function listComponents(
  tappId: string,
  type?: ComponentType,
  runtimeGrant?: string,
): Promise<{ success: boolean; components: RegisteredComponent[] }> {
  const url = type
    ? `/api/tapp/components/${encodeURIComponent(tappId)}?type=${type}`
    : `/api/tapp/components/${encodeURIComponent(tappId)}`
  return apiRequest(url, { runtimeGrant })
}

/**
 * 列出所有指定类型的组件
 */
export async function listAllComponentsByType(
  componentType: ComponentType,
  runtimeGrant?: string,
): Promise<{
  success: boolean
  type: string
  components: RegisteredComponent[]
}> {
  return apiRequest(`/api/tapp/components/all/${componentType}`, {
    runtimeGrant,
  })
}

// ============ P2: Shortcut Registration API ============

/** 快捷键配置 */
export interface ShortcutConfig {
  id: string
  keys: string
  description: string
  action: string
  scope?: 'global' | 'tapp' | 'editor'
}

/** 已注册快捷键 */
export interface RegisteredShortcut {
  id: string
  tappId: string
  keys: string
  description: string
  action: string
  scope: string
  registeredAt: string
  enabled: boolean
}

/**
 * 注册快捷键
 */
export async function registerShortcut(
  tappId: string,
  config: ShortcutConfig,
  runtimeGrant?: string,
): Promise<{ success: boolean; shortcut: RegisteredShortcut }> {
  return apiRequest('/api/tapp/shortcuts/register', {
    method: 'POST',
    body: JSON.stringify({
      tapp_id: tappId,
      shortcut_id: config.id,
      keys: config.keys,
      description: config.description,
      action: config.action,
      scope: config.scope,
    }),
    runtimeGrant,
  })
}

/**
 * 注销快捷键
 */
export async function unregisterShortcut(
  tappId: string,
  shortcutId: string,
  runtimeGrant?: string,
): Promise<{ success: boolean; unregistered: string }> {
  return apiRequest(
    `/api/tapp/shortcuts/${encodeURIComponent(tappId)}/${encodeURIComponent(shortcutId)}`,
    {
      method: 'DELETE',
      runtimeGrant,
    },
  )
}

/**
 * 列出快捷键
 */
export async function listShortcuts(
  tappId?: string,
  runtimeGrant?: string,
): Promise<{ success: boolean; shortcuts: RegisteredShortcut[] }> {
  const url = tappId
    ? `/api/tapp/shortcuts?tapp_id=${encodeURIComponent(tappId)}`
    : '/api/tapp/shortcuts'
  return apiRequest(url, { runtimeGrant })
}

// ============ Event Broker API ============

export async function publishEvent(
  request: PublishEventRequest,
  runtimeGrant: string,
): Promise<{
  accepted: boolean
  deduplicated: boolean
  delivered: number
  event: TappEvent
}> {
  return apiRequest('/api/tapp/events/publish', {
    method: 'POST',
    body: JSON.stringify(request),
    runtimeGrant,
  })
}

export async function streamEvents(
  runtimeGrant: string,
  onEvent: (event: string, data: unknown) => void,
  signal?: AbortSignal,
): Promise<void> {
  return streamRuntimeEvents(
    '/api/tapp/events/stream',
    runtimeGrant,
    onEvent,
    signal,
  )
}

export async function streamAgentInteractions(
  runtimeGrant: string,
  onInteraction: (interaction: AgentInteractionV2) => void,
  signal?: AbortSignal,
): Promise<void> {
  return streamRuntimeEvents(
    '/api/tapp/agent/v2/interactions/stream',
    runtimeGrant,
    (event, data) => {
      if (event === 'interaction' && data && typeof data === 'object') {
        onInteraction(data as AgentInteractionV2)
      }
    },
    signal,
  )
}

export async function getAgentInteraction(
  interactionId: string,
  runtimeGrant: string,
): Promise<AgentInteractionV2> {
  return apiRequest(
    `/api/tapp/agent/v2/interactions/${encodeURIComponent(interactionId)}`,
    { runtimeGrant },
  )
}

export async function acceptAgentInteraction(
  interactionId: string,
  runtimeGrant: string,
): Promise<AgentInteractionV2> {
  return apiRequest(
    `/api/tapp/agent/v2/interactions/${encodeURIComponent(interactionId)}/accept`,
    { method: 'POST', runtimeGrant },
  )
}

export async function submitAgentInteractionResult(
  interactionId: string,
  result: { data: unknown; summary?: string; idempotencyKey: string },
  runtimeGrant: string,
): Promise<AgentInteractionV2> {
  return apiRequest(
    `/api/tapp/agent/v2/interactions/${encodeURIComponent(interactionId)}/result`,
    { method: 'POST', body: JSON.stringify(result), runtimeGrant },
  )
}

export async function rejectAgentInteraction(
  interactionId: string,
  reason: string,
  runtimeGrant: string,
): Promise<AgentInteractionV2> {
  return apiRequest(
    `/api/tapp/agent/v2/interactions/${encodeURIComponent(interactionId)}/reject`,
    { method: 'POST', body: JSON.stringify({ reason }), runtimeGrant },
  )
}

export async function requestAgentIntent(
  interactionId: string,
  request: { type: string; params?: unknown; reason: string },
  runtimeGrant: string,
): Promise<unknown> {
  return apiRequest(
    `/api/tapp/agent/v2/interactions/${encodeURIComponent(interactionId)}/intents`,
    {
      method: 'POST',
      body: JSON.stringify({ ...request, hostConfirmed: true }),
      runtimeGrant,
    },
  )
}
