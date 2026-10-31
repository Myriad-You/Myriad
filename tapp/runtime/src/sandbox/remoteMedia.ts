/**
 * media:remote 域名规则：CSP host-source 与包装器 `_isAllowedImageUrl` 共用。
 * 只吃后端算好的授予域名（声明 ∩ 安装批准），不读 manifest。
 */

// 与 crates/tapp-contract `validate_remote_media_host` 同一字符集。不合格的条目直接丢弃，
// 绝不拼进 CSP（引号、空格、分号都会改写策略）。
const LABEL = '[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?'
const HOST_PATTERN = new RegExp(String.raw`^(?:\*\.)?${LABEL}(?:\.${LABEL})+$`)

export function sanitizeRemoteMediaHosts(hosts: readonly unknown[] | undefined): string[] {
  if (!Array.isArray(hosts)) return []
  const out: string[] = []
  for (const host of hosts) {
    if (
      typeof host === 'string' &&
      host.length <= 255 &&
      HOST_PATTERN.test(host) &&
      !out.includes(host)
    ) {
      out.push(host)
    }
  }
  return out
}

/**
 * 沙箱内求值的源码。按 CSP 语义：只认 https、默认端口；`*.x.com` 只配子域，不含 `x.com`。
 */
export const REMOTE_MEDIA_MATCH_SOURCE = `
function isRemoteMediaUrlAllowed(value, hosts) {
  if (!hosts || hosts.length === 0) return false;
  let raw = String(value).trim();
  if (raw.startsWith('//')) raw = 'https:' + raw;
  let url;
  try { url = new URL(raw); } catch { return false; }
  if (url.protocol !== 'https:' || url.port !== '' || url.username || url.password) return false;
  const host = url.hostname.toLowerCase().replace(/\\.$/, '');
  for (const pattern of hosts) {
    if (pattern.startsWith('*.')) {
      const suffix = pattern.slice(1);
      if (host.length > suffix.length && host.endsWith(suffix)) return true;
    } else if (host === pattern) {
      return true;
    }
  }
  return false;
}
`
