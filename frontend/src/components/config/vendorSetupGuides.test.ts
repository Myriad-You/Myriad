import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { getVendorSetupGuide } from './vendorSetupGuides'

const t = {
  aiVendorSetupTitle: '配置方法',
  aiVendorSetupOpen: '打开',
  aiVendorSetupPortalTitle: '打开 {name} 控制台',
  aiVendorSetupPortalDesc: '登录后台。',
  aiVendorSetupCreateTitle: '创建并复制密钥',
  aiVendorSetupCreateDesc: '新建一把 API Key。',
  aiVendorSetupFillTitle: '回到本页填写',
  aiVendorSetupFillDesc: '贴到 API Key。',
  aiVendorSetupAzurePortalTitle: '打开 Azure 门户',
  aiVendorSetupAzurePortalDesc: '进入资源。',
  aiVendorSetupAzureCreateTitle: '复制密钥和地址',
  aiVendorSetupAzureCreateDesc: '复制 Key 与 Endpoint。',
  aiVendorSetupAzureFillTitle: '回到本页填写',
  aiVendorSetupAzureFillDesc: '填 Key 和 Base URL。',
  aiVendorSetupCompatiblePortalTitle: '准备兼容接口',
  aiVendorSetupCompatiblePortalDesc: '确认 /v1。',
  aiVendorSetupCompatibleCreateTitle: '复制密钥',
  aiVendorSetupCompatibleCreateDesc: '复制 API Key。',
  aiVendorSetupCompatibleFillTitle: '回到本页填写',
  aiVendorSetupCompatibleFillDesc: '填 Key 和 URL。',
  aiVendorSetupOllamaPortalTitle: '安装 Ollama',
  aiVendorSetupOllamaPortalDesc: '本机安装。',
  aiVendorSetupOllamaCreateTitle: '拉取模型',
  aiVendorSetupOllamaCreateDesc: 'ollama pull。',
  aiVendorSetupOllamaFillTitle: '回到本页填写',
  aiVendorSetupOllamaFillDesc: '可填占位。',
  aiVendorSetupTencentPortalTitle: '打开腾讯云密钥',
  aiVendorSetupTencentPortalDesc: '访问管理。',
  aiVendorSetupTencentCreateTitle: '创建密钥对',
  aiVendorSetupTencentCreateDesc: '复制 SecretId。',
  aiVendorSetupTencentFillTitle: '回到本页填写',
  aiVendorSetupTencentFillDesc: '填两项。',
}

describe('vendor setup guides', () => {
  it('opens the vendor console then asks to paste the key', () => {
    const guide = getVendorSetupGuide(
      { preset: 'openrouter', display_name: 'OpenRouter' },
      t,
    )
    assert.ok(guide)
    assert.equal(guide.title, '配置方法')
    assert.equal(guide.steps[0].href, 'https://openrouter.ai/keys')
    assert.equal(guide.steps[0].title, '打开 OpenRouter 控制台')
    assert.equal(guide.steps[2].title, '回到本页填写')
  })

  it('uses Tencent CAM steps for speech', () => {
    const guide = getVendorSetupGuide({ preset: 'tencent', kind: 'tencent' }, t)
    assert.ok(guide)
    assert.equal(guide.steps[0].href, 'https://console.cloud.tencent.com/cam/capi')
    assert.equal(guide.steps[0].title, '打开腾讯云密钥')
  })

  it('asks Azure for key and endpoint', () => {
    const guide = getVendorSetupGuide(
      { preset: 'azureOpenAI', display_name: 'Azure OpenAI' },
      t,
    )
    assert.ok(guide)
    assert.equal(guide.steps[2].description, '填 Key 和 Base URL。')
  })
})
