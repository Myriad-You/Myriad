# 出站 http 端点域名固定为 allowlist

Status: accepted

现状是出站层只挡内网 IP，任意公网域都放行；只有 credential 绑定的端点要求固定 HTTPS origin，非 credential 端点的域名可以随意写、甚至用模板变量注入域名。

决定：所有 http 端点的 scheme 与 host 必须写死 HTTPS，模板变量只允许出现在 path 与 query。域名本身即 allowlist——manifest 声明了哪个域名，就只允许访问哪个域名。

## 理由

- 堵住 SSRF：模板不能注入域名，杜绝「应用拼出任意域交给平台代理」这条路。
- 复用 credential 端点已有的 fixed-origin 校验，推广到所有 http 端点，不新增 manifest 字段。

## Consequences

代价是 `https://{{region}}.example.com` 这类「host 也走模板」的写法被禁止。这类场景少见，且正是 SSRF 的高危面。
