/**
 * 认证状态的模块级镜像 —— 供 React 之外的代码读取。
 *
 * 为什么需要它：Tapp 沙箱桥接（FederationBridge 等）不是组件，拿不到
 * AuthContext，却需要知道「当前访客到底登没登录」才能避免把 user-scoped
 * 的联邦调用原样转发出去（访客调用只可能拿到 401）。
 *
 * ⚠️ 与 utils/sessionDetection 的区别：那个是 localStorage 里的**启发式**
 * 提示，文件里明确写了「不用于权限判断」。这里存的是 /api/auth/me 的
 * **权威**结论，只由 AuthContext 在拿到确定答案时写入。
 *
 * 三态而非布尔 —— `null` 表示「还不知道」：首屏探测未回、5xx、网络超时
 * 都停在 null。所有消费方必须 fail-open（未知即放行），宁可多发一个请求，
 * 也绝不能因为状态没就绪而误挡真实用户。
 *
 * 这里**不做**权限判断，只做「省掉必然失败的请求」的优化。真正的鉴权
 * 始终在后端。
 */

/** `true` 已登录 · `false` 确定是访客 · `null` 尚不确定 */
export type KnownAuthState = boolean | null

let knownAuthState: KnownAuthState = null

/** 是否**确定**为未登录访客（`null` 一律返回 false，即不确定就不算访客） */
export function isKnownGuest(): boolean {
  return knownAuthState === false
}

/**
 * 由 AuthContext 在 /api/auth/me 给出确定结论时写入。
 *
 * 网络错误 / 5xx 不要调用本函数——那些情况应保持未知（干脆不动）。
 * 登出写入 `false`（确定访客），不要回到 `null`。
 */
export function setKnownAuthState(state: boolean): void {
  knownAuthState = state
}
