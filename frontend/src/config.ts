/** Empty PUBLIC_API_URL → same-origin /api (dev proxy / prod CSP). */
const configuredApiUrl = (import.meta.env?.PUBLIC_API_URL || '').trim()
export const API_URL = configuredApiUrl
