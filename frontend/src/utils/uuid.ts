/**
 * `crypto.randomUUID` 只在安全上下文（HTTPS / localhost）可用。
 * 局域网用 `http://192.168.x.x` 访问时它不存在，直接调用会在模块加载阶段抛
 * TypeError，把整棵树打进启动兜底页。这里给一个等价退路。
 */
export function safeRandomUUID(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  // getRandomValues 不受安全上下文限制，按 RFC 4122 v4 自行拼装。
  const bytes = crypto.getRandomValues(new Uint8Array(16))
  bytes[6] = (bytes[6] & 0x0F) | 0x40
  bytes[8] = (bytes[8] & 0x3F) | 0x80
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}
