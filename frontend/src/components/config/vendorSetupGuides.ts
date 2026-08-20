/**
 * 服务商配置步骤。文案走 i18n；控制台链接按预设 id。
 */

import type { SetupFlowStep } from '../settings/SetupFlow'
import { findVendorPreset } from './aiVendorPresets'

export interface VendorSetupI18n {
  aiVendorSetupTitle: string
  aiVendorSetupOpen: string
  aiVendorSetupPortalTitle: string
  aiVendorSetupPortalDesc: string
  aiVendorSetupCreateTitle: string
  aiVendorSetupCreateDesc: string
  aiVendorSetupFillTitle: string
  aiVendorSetupFillDesc: string
  aiVendorSetupAzurePortalTitle: string
  aiVendorSetupAzurePortalDesc: string
  aiVendorSetupAzureCreateTitle: string
  aiVendorSetupAzureCreateDesc: string
  aiVendorSetupAzureFillTitle: string
  aiVendorSetupAzureFillDesc: string
  aiVendorSetupCompatiblePortalTitle: string
  aiVendorSetupCompatiblePortalDesc: string
  aiVendorSetupCompatibleCreateTitle: string
  aiVendorSetupCompatibleCreateDesc: string
  aiVendorSetupCompatibleFillTitle: string
  aiVendorSetupCompatibleFillDesc: string
  aiVendorSetupOllamaPortalTitle: string
  aiVendorSetupOllamaPortalDesc: string
  aiVendorSetupOllamaCreateTitle: string
  aiVendorSetupOllamaCreateDesc: string
  aiVendorSetupOllamaFillTitle: string
  aiVendorSetupOllamaFillDesc: string
  aiVendorSetupTencentPortalTitle: string
  aiVendorSetupTencentPortalDesc: string
  aiVendorSetupTencentCreateTitle: string
  aiVendorSetupTencentCreateDesc: string
  aiVendorSetupTencentFillTitle: string
  aiVendorSetupTencentFillDesc: string
}

const CONSOLE_URL: Record<string, string> = {
  openrouter: 'https://openrouter.ai/keys',
  openai: 'https://platform.openai.com/api-keys',
  azureOpenAI: 'https://portal.azure.com/',
  gemini: 'https://aistudio.google.com/apikey',
  anthropic: 'https://console.anthropic.com/settings/keys',
  deepseek: 'https://platform.deepseek.com/api_keys',
  volcengine: 'https://console.volcengine.com/ark',
  dashscope: 'https://bailian.console.aliyun.com/',
  moonshot: 'https://platform.moonshot.cn/console/api-keys',
  zhipu: 'https://open.bigmodel.cn/usercenter/apikeys',
  siliconflow: 'https://cloud.siliconflow.cn/account/ak',
  groq: 'https://console.groq.com/keys',
  xai: 'https://console.x.ai/',
  mistral: 'https://console.mistral.ai/api-keys',
  together: 'https://api.together.ai/settings/api-keys',
  fireworks: 'https://fireworks.ai/account/api-keys',
  perplexity: 'https://www.perplexity.ai/settings/api',
  minimax:
    'https://platform.minimaxi.com/user-center/basic-information/interface-key',
  ollama: 'https://github.com/ollama/ollama',
  cloudflare: 'https://dash.cloudflare.com/',
  cohere: 'https://dashboard.cohere.com/api-keys',
  nvidia: 'https://build.nvidia.com/settings/api-key',
  tencentHunyuan: 'https://console.cloud.tencent.com/hunyuan',
  tencent: 'https://console.cloud.tencent.com/cam/capi',
}

function fillName(template: string, name: string): string {
  return template.replaceAll('{name}', name)
}

function setupKind(presetId: string): 'azure' | 'compatible' | 'ollama' | 'tencent' | 'key' {
  if (presetId === 'azureOpenAI') return 'azure'
  if (presetId === 'openaiCompatible') return 'compatible'
  if (presetId === 'ollama') return 'ollama'
  if (presetId === 'tencent') return 'tencent'
  return 'key'
}

export function getVendorSetupGuide(
  source: { preset?: string | null; slug?: string; kind?: string; display_name?: string },
  t: VendorSetupI18n,
): { title: string; steps: SetupFlowStep[] } | null {
  const preset = findVendorPreset(source)
  const presetId = preset?.id || source.preset?.trim() || ''
  if (!presetId && !source.kind) return null
  const name = (source.display_name || preset?.display_name || presetId).trim()
  const href = (presetId && CONSOLE_URL[presetId]) || preset?.docs_url
  const kind = setupKind(presetId || source.kind || 'key')
  const open = t.aiVendorSetupOpen

  const step = (
    key: string,
    title: string,
    description: string,
    link?: string,
  ): SetupFlowStep => ({
    key,
    title: fillName(title, name),
    description: fillName(description, name),
    href: link,
    actionLabel: link ? open : undefined,
  })

  if (kind === 'azure') {
    return {
      title: t.aiVendorSetupTitle,
      steps: [
        step('portal', t.aiVendorSetupAzurePortalTitle, t.aiVendorSetupAzurePortalDesc, href),
        step('create', t.aiVendorSetupAzureCreateTitle, t.aiVendorSetupAzureCreateDesc),
        step('fill', t.aiVendorSetupAzureFillTitle, t.aiVendorSetupAzureFillDesc),
      ],
    }
  }
  if (kind === 'compatible') {
    return {
      title: t.aiVendorSetupTitle,
      steps: [
        step(
          'portal',
          t.aiVendorSetupCompatiblePortalTitle,
          t.aiVendorSetupCompatiblePortalDesc,
        ),
        step('create', t.aiVendorSetupCompatibleCreateTitle, t.aiVendorSetupCompatibleCreateDesc),
        step('fill', t.aiVendorSetupCompatibleFillTitle, t.aiVendorSetupCompatibleFillDesc),
      ],
    }
  }
  if (kind === 'ollama') {
    return {
      title: t.aiVendorSetupTitle,
      steps: [
        step('portal', t.aiVendorSetupOllamaPortalTitle, t.aiVendorSetupOllamaPortalDesc, href),
        step('create', t.aiVendorSetupOllamaCreateTitle, t.aiVendorSetupOllamaCreateDesc),
        step('fill', t.aiVendorSetupOllamaFillTitle, t.aiVendorSetupOllamaFillDesc),
      ],
    }
  }
  if (kind === 'tencent') {
    return {
      title: t.aiVendorSetupTitle,
      steps: [
        step('portal', t.aiVendorSetupTencentPortalTitle, t.aiVendorSetupTencentPortalDesc, href),
        step('create', t.aiVendorSetupTencentCreateTitle, t.aiVendorSetupTencentCreateDesc),
        step('fill', t.aiVendorSetupTencentFillTitle, t.aiVendorSetupTencentFillDesc),
      ],
    }
  }
  return {
    title: t.aiVendorSetupTitle,
    steps: [
      step('portal', t.aiVendorSetupPortalTitle, t.aiVendorSetupPortalDesc, href),
      step('create', t.aiVendorSetupCreateTitle, t.aiVendorSetupCreateDesc),
      step('fill', t.aiVendorSetupFillTitle, t.aiVendorSetupFillDesc),
    ],
  }
}
