export const AI_REQUEST_TIMEOUT_FLOOR_MS: number
export const AI_IMAGE_REQUEST_TIMEOUT_MS: number
export function requestPathname(urlPath: unknown): string
export function aiRequestTimeoutMs(url: string): number | undefined
export function withAiTimeoutSignal(url: string, init?: RequestInit): RequestInit
