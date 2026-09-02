/**
 * 「别看这一页」开关。
 *
 * 助手每次请求都默认捎上当前页面的正文。这在自己的站点上是方便，但既然界面已经
 * 说出了「我现在看得到什么」，就得同时给出关掉的办法 —— 只告知不给关，等于通知
 * 而不是许可。
 *
 * 只管页面正文这一路。选中的文字是用户自己划出来的，而且会原样出现在发出去的
 * 那句话里，看得见，不需要另外一道闸。
 *
 * 存在本地：这是「我这台设备上的习惯」，不是账号设置，不值得为它开一条服务端配置。
 */

const STORAGE_KEY = 'myriad.agentPanel.contextConsent'

let consent = true
let loaded = false
const listeners = new Set<() => void>()

function read(): boolean {
  try {
    // 没存过就是默认开着：站点助手读当前页是它的本职
    return window.localStorage.getItem(STORAGE_KEY) !== 'off'
  } catch {
    // 隐私模式下读不到 storage，别因此把功能关掉
    return true
  }
}

function ensureLoaded(): void {
  if (loaded) return
  loaded = true
  if (typeof window !== 'undefined') consent = read()
}

export function getAgentContextConsent(): boolean {
  ensureLoaded()
  return consent
}

export function setAgentContextConsent(next: boolean): void {
  ensureLoaded()
  if (consent === next) return
  consent = next
  try {
    window.localStorage.setItem(STORAGE_KEY, next ? 'on' : 'off')
  } catch {
    // 存不下就只在这一次会话里生效，不该因此拒绝切换
  }
  for (const listener of listeners) listener()
}

export function subscribeAgentContextConsent(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function getServerAgentContextConsent(): boolean {
  return true
}
