/**
 * AI 配置页「快捷访问」：服务商独立指南（中文）
 * 每条均为完整 SettingGuideEntry，由 SettingGuideBody 渲染。
 */
import type { SettingGuideEntry } from './types'

export const aiProvidersQuickAccessZh: SettingGuideEntry = {
  what: '常见 AI 服务商速查：在哪里申请 API Key、官方文档与默认接口地址。',
  chain:
    '① 点本页右上角「快捷访问」打开本指南。\n② 按服务商条目找到密钥页 / 文档 / API 地址。\n③ 回到本页选对应服务商（或「OpenAI 兼容」+ Base URL），粘贴密钥与模型名 → 底部保存。\n④ 用助手发一句简单话做连通测试。',
  frontend:
    '设置 → AI 配置 → 标题栏「重置」与「显示说明」之间的「快捷访问」。\n各档位（标准 / 轻量 / 高质量 / 图片）可选用不同服务商。',
  notes:
    '密钥只保存在服务器。浏览器能打开官网 ≠ 服务器一定能连上（需时在「高级」配代理）。\n换服务商后务必核对模型名。下列多数兼容 OpenAI 协议的服务商，在本页选「OpenAI 兼容」并填写其 Base URL 即可。',
}

export const aiProvidersZh: Record<string, SettingGuideEntry> = {
  openrouter: {
    what: 'OpenRouter：聚合多家模型的统一网关（推荐上手）。',
    chain:
      '① 打开密钥页注册/登录：https://openrouter.ai/keys\n② 创建 API Key 并复制。\n③ 本页服务商选 OpenRouter（或 OpenAI 兼容 + Base URL）。\n④ 模型名用控制台/文档中的写法（如 openai/gpt-4o、anthropic/claude-…）。\n⑤ 文档：https://openrouter.ai/docs\n⑥ 默认 API：https://openrouter.ai/api/v1',
    frontend: '标准 / 轻量 / 高质量 / 图片均可选 OpenRouter。',
    notes: '一种 Key 可路由多家上游；费用与限流以 OpenRouter 账户为准。',
  },
  openai: {
    what: 'OpenAI 官方 API（ChatGPT 模型、Images 等）。',
    chain:
      '① 密钥：https://platform.openai.com/api-keys\n② 文档：https://platform.openai.com/docs\n③ 默认 API：https://api.openai.com/v1\n④ 本页选 OpenAI 兼容 / OpenAI，填密钥与模型（如 gpt-4.1、gpt-5.6-terra）。\n⑤ 用量与账单：https://platform.openai.com/usage',
    frontend: '文本各档与图片生成均可使用。',
    notes: '需已开通对应模型权限；部分地区需合规与支付方式。',
  },
  azureOpenAI: {
    what: 'Azure OpenAI：微软云上的 OpenAI 兼容端点。',
    chain:
      '① 门户与资源：https://portal.azure.com\n② 文档：https://learn.microsoft.com/azure/ai-services/openai/\n③ 在 Azure 创建资源与部署，复制 endpoint 与 key。\n④ 本页选「OpenAI 兼容」，Base URL 填你的 Azure 端点（通常含 /openai/deployments/... 或资源根路径，按官方当前格式）。\n⑤ 模型名填部署名（deployment name）。',
    frontend: '与 OpenAI 兼容字段相同；模型名=部署名。',
    notes: '区域、配额与密钥轮换在 Azure 门户管理；格式随微软文档更新。',
  },
  gemini: {
    what: 'Google Gemini（AI Studio / Gemini API）。',
    chain:
      '① 密钥：https://aistudio.google.com/apikey\n② 文档：https://ai.google.dev/gemini-api/docs\n③ 本页服务商选 Gemini，填密钥。\n④ 模型名以 Google 文档为准（如 gemini-2.5-flash）。',
    frontend: '文本档位选 Gemini；高级里可有全局 Gemini Base URL 覆盖。',
    notes: '与 OpenAI 协议不同，不要用 OpenAI 兼容填 Gemini 密钥。',
  },
  anthropic: {
    what: 'Anthropic Claude 官方 API。',
    chain:
      '① 控制台：https://console.anthropic.com/\n② 密钥：https://console.anthropic.com/settings/keys\n③ 文档：https://docs.anthropic.com/\n④ 本站常见用法：经 OpenRouter 选 anthropic/… 模型；或自建 OpenAI 兼容中转。\n⑤ 原生 API 非 OpenAI 协议，直连需中转层。',
    frontend: '推荐 OpenRouter 或兼容中转；模型名见 Anthropic/OpenRouter 文档。',
    notes: '勿把 Anthropic 密钥当 OpenAI key 填进未做协议转换的端点。',
  },
  deepseek: {
    what: 'DeepSeek 深度求索（OpenAI 兼容）。',
    chain:
      '① 开放平台：https://platform.deepseek.com/\n② 密钥：https://platform.deepseek.com/api_keys\n③ 文档：https://api-docs.deepseek.com/\n④ API：https://api.deepseek.com 或 https://api.deepseek.com/v1\n⑤ 本页选 OpenAI 兼容，Base URL 填上述地址，模型如 deepseek-chat、deepseek-reasoner。',
    frontend: '标准/轻量/Pro 选 OpenAI 兼容并填 DeepSeek Base URL。',
    notes: '模型名以控制台为准；注意上下文长度与价格档。',
  },
  volcengine: {
    what: '火山引擎方舟（豆包等；本站图片生成可选）。',
    chain:
      '① 控制台：https://console.volcengine.com/ark\n② 文档：https://www.volcengine.com/docs/82379\n③ 创建推理接入点，复制 API Key。\n④ 文本：可用 OpenAI 兼容 + 方舟 Base URL（见官方「OpenAI SDK 兼容」说明）。\n⑤ 图片：本页「图片生成」选火山引擎，填 Key。',
    frontend: '图片分组有火山专用项；文本多走 OpenAI 兼容。',
    notes: '接入点 ID / 模型名与官方控制台一致；国内网络通常更稳。',
  },
  dashscope: {
    what: '阿里云百炼 / DashScope（通义千问，OpenAI 兼容）。',
    chain:
      '① 控制台：https://bailian.console.aliyun.com/\n② API-KEY：https://bailian.console.aliyun.com/#/api-key\n③ 文档：https://help.aliyun.com/zh/model-studio/\n④ 兼容模式 Base URL 示例：https://dashscope.aliyuncs.com/compatible-mode/v1\n⑤ 本页选 OpenAI 兼容，模型如 qwen-plus、qwen-max（以控制台为准）。',
    frontend: 'OpenAI 兼容 + DashScope Base URL。',
    notes: '国际站与中国站 endpoint 可能不同，按账号地域选文档。',
  },
  moonshot: {
    what: 'Moonshot 月之暗面（Kimi，OpenAI 兼容）。',
    chain:
      '① 开放平台：https://platform.moonshot.cn/\n② 密钥在控制台 API Key 页创建。\n③ 文档：https://platform.moonshot.cn/docs\n④ API：https://api.moonshot.cn/v1\n⑤ 本页选 OpenAI 兼容，模型如 moonshot-v1-8k / kimi 系列（以文档为准）。',
    frontend: 'OpenAI 兼容 + Moonshot Base URL。',
    notes: '长上下文模型注意单价与 max tokens。',
  },
  zhipu: {
    what: '智谱开放平台（GLM，OpenAI 兼容）。',
    chain:
      '① 开放平台：https://open.bigmodel.cn/\n② 密钥：https://open.bigmodel.cn/usercenter/apikeys\n③ 文档：https://docs.bigmodel.cn/\n④ 兼容 API 示例：https://open.bigmodel.cn/api/paas/v4\n⑤ 本页选 OpenAI 兼容，模型如 glm-4、glm-4-flash。',
    frontend: 'OpenAI 兼容 + 智谱 Base URL。',
    notes: '具体 path 以智谱当前 OpenAI 兼容文档为准。',
  },
  siliconflow: {
    what: '硅基流动 SiliconFlow（聚合模型，OpenAI 兼容）。',
    chain:
      '① 控制台：https://cloud.siliconflow.cn/\n② 密钥在账户 API 密钥中创建。\n③ 文档：https://docs.siliconflow.cn/\n④ API：https://api.siliconflow.cn/v1\n⑤ 本页选 OpenAI 兼容，模型名从控制台模型广场复制。',
    frontend: 'OpenAI 兼容 + SiliconFlow Base URL。',
    notes: '免费/付费模型混用时注意限流与计费。',
  },
  groq: {
    what: 'Groq：高速推理（OpenAI 兼容）。',
    chain:
      '① 控制台：https://console.groq.com/\n② 密钥：https://console.groq.com/keys\n③ 文档：https://console.groq.com/docs\n④ API：https://api.groq.com/openai/v1\n⑤ 本页选 OpenAI 兼容，模型如 llama-3.3-70b-versatile（以文档为准）。',
    frontend: 'OpenAI 兼容 + Groq Base URL。',
    notes: '免费档有 RPM 限制；生产环境关注配额。',
  },
  xai: {
    what: 'xAI Grok API（OpenAI 兼容）。',
    chain:
      '① 控制台：https://console.x.ai/\n② 文档：https://docs.x.ai/\n③ API：https://api.x.ai/v1\n④ 创建 API Key 后，本页选 OpenAI 兼容，模型如 grok-3、grok-2（以文档为准）。',
    frontend: 'OpenAI 兼容 + xAI Base URL。',
    notes: '模型与区域可用性以 xAI 控制台为准。',
  },
  mistral: {
    what: 'Mistral AI（OpenAI 兼容）。',
    chain:
      '① 控制台：https://console.mistral.ai/\n② 密钥：https://console.mistral.ai/api-keys/\n③ 文档：https://docs.mistral.ai/\n④ API：https://api.mistral.ai/v1\n⑤ 本页选 OpenAI 兼容，模型如 mistral-large-latest、ministral-…。',
    frontend: 'OpenAI 兼容 + Mistral Base URL。',
    notes: '欧洲数据驻留需求可看 Mistral 企业文档。',
  },
  together: {
    what: 'Together AI：开源模型托管（OpenAI 兼容）。',
    chain:
      '① 控制台：https://api.together.xyz/\n② 密钥：https://api.together.xyz/settings/api-keys\n③ 文档：https://docs.together.ai/\n④ API：https://api.together.xyz/v1\n⑤ 本页选 OpenAI 兼容，模型名从 Together 模型列表复制。',
    frontend: 'OpenAI 兼容 + Together Base URL。',
    notes: '模型 ID 通常较长，完整粘贴。',
  },
  fireworks: {
    what: 'Fireworks AI（OpenAI 兼容）。',
    chain:
      '① 控制台：https://fireworks.ai/\n② 密钥在 Account → API keys。\n③ 文档：https://docs.fireworks.ai/\n④ API：https://api.fireworks.ai/inference/v1\n⑤ 本页选 OpenAI 兼容，模型 accounts/…/models/… 格式以文档为准。',
    frontend: 'OpenAI 兼容 + Fireworks Base URL。',
    notes: '注意 inference 路径与模型命名空间。',
  },
  perplexity: {
    what: 'Perplexity API（OpenAI 兼容聊天）。',
    chain:
      '① 设置与密钥：https://www.perplexity.ai/settings/api\n② 文档：https://docs.perplexity.ai/\n③ API：https://api.perplexity.ai\n④ 本页选 OpenAI 兼容，模型如 sonar、sonar-pro（以文档为准）。',
    frontend: 'OpenAI 兼容 + Perplexity Base URL。',
    notes: '偏搜索增强回答；计费与网页版套餐可能分离。',
  },
  minimax: {
    what: 'MiniMax 海螺（OpenAI 兼容）。',
    chain:
      '① 开放平台：https://platform.minimaxi.com/\n② 文档：https://platform.minimaxi.com/document/\n③ 创建 Group / API Key。\n④ 兼容 Base URL 与模型名见官方「OpenAI API 兼容」章节。\n⑤ 本页选 OpenAI 兼容并填写。',
    frontend: 'OpenAI 兼容 + MiniMax Base URL。',
    notes: '国际站与国内站域名可能不同。',
  },
  ollama: {
    what: 'Ollama：本机/内网本地模型（OpenAI 兼容）。',
    chain:
      '① 安装与文档：https://ollama.com/ 与 https://github.com/ollama/ollama\n② 默认本机 API：http://127.0.0.1:11434/v1\n③ 先 ollama pull 模型，再在本页选 OpenAI 兼容，Base URL 填上述地址。\n④ 模型名即本地模型标签（如 llama3.2）。\n⑤ 密钥可填任意非空占位（若后端要求必填）。',
    frontend: 'OpenAI 兼容；服务器须能访问 Ollama 所在主机。',
    notes: 'Myriad 跑在远程服务器时，127.0.0.1 是服务器自己，不是你的电脑；请用内网 IP 或隧道。',
  },
  cloudflare: {
    what: 'Cloudflare Workers AI（OpenAI 兼容网关）。',
    chain:
      '① 控制台：https://dash.cloudflare.com/\n② 文档：https://developers.cloudflare.com/workers-ai/\n③ 使用 AI Gateway / Workers AI OpenAI 兼容端点（文档中的 base URL）。\n④ 本页选 OpenAI 兼容，填 Account 相关 endpoint 与 API Token。',
    frontend: 'OpenAI 兼容 + Cloudflare 文档中的 Base URL。',
    notes: '模型 ID 为 Cloudflare 目录中的名称。',
  },
  cohere: {
    what: 'Cohere（Chat API；可用兼容层或官方 SDK）。',
    chain:
      '① 控制台：https://dashboard.cohere.com/\n② 密钥：https://dashboard.cohere.com/api-keys\n③ 文档：https://docs.cohere.com/\n④ 若使用 OpenAI 兼容代理/网关则填网关 Base URL；否则多经 OpenRouter 的 cohere/… 模型。',
    frontend: '优先 OpenRouter 或兼容网关。',
    notes: '原生协议与 OpenAI 不完全相同。',
  },
  nvidia: {
    what: 'NVIDIA NIM / NGC 推理（OpenAI 兼容）。',
    chain:
      '① 目录与文档：https://build.nvidia.com/ 与 https://docs.nvidia.com/nim/\n② 获取 API Key 后使用官方给出的 OpenAI 兼容 endpoint。\n③ 本页选 OpenAI 兼容，模型名从 build.nvidia.com 复制。',
    frontend: 'OpenAI 兼容 + NVIDIA 提供的 Base URL。',
    notes: '部分模型需接受条款后才可调用。',
  },
  tencentHunyuan: {
    what: '腾讯混元大模型（云 API / 兼容接口）。',
    chain:
      '① 控制台：https://console.cloud.tencent.com/hunyuan\n② 文档：https://cloud.tencent.com/document/product/1729\n③ 创建密钥或兼容模式 endpoint（见当前文档）。\n④ 若提供 OpenAI 兼容地址：本页选 OpenAI 兼容并填写；否则使用官方 SDK/网关。',
    frontend: '有兼容 endpoint 时走 OpenAI 兼容字段。',
    notes: '语音相关仍可用本页腾讯云 TTS/ASR 独立配置。',
  },
  baiduQianfan: {
    what: '百度智能云千帆（文心等）。',
    chain:
      '① 控制台：https://console.bce.baidu.com/qianfan/\n② 文档：https://cloud.baidu.com/doc/WENXINWORKSHOP/index.html\n③ 创建应用获取 API Key / Secret 或兼容模式凭证。\n④ 若文档提供 OpenAI 兼容 Base URL，本页选 OpenAI 兼容；否则需专用网关。',
    frontend: '兼容模式下同其它 OpenAI 兼容服务商。',
    notes: '鉴权方式随千帆版本变化，以最新文档为准。',
  },
  openaiCompatible: {
    what: '任意 OpenAI 兼容中转 / 聚合（One-API、New API、自建网关等）。',
    chain:
      '① 向中转管理员索取：API Base URL、API Key、可用模型名列表。\n② 本页服务商选「OpenAI 兼容」。\n③ Base URL 一般以 /v1 结尾（按中转说明，不要多写 /chat/completions）。\n④ 模型名必须与中转后台已配置的上游一致。\n⑤ 保存后先小流量测试。',
    frontend: '所有文本档位的 OpenAI 兼容表单。',
    notes: '中转泄露风险自负；不要使用来路不明的公益中转处理敏感数据。',
  },
}
