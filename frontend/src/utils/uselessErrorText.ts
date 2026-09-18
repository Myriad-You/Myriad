export function isUselessErrorText(text: string): boolean {
  const detail = text.replaceAll(/\s+/g, ' ').trim()
  if (!detail) return true
  if (/^API Error:\s*\d+$/i.test(detail)) return true
  if (/^HTTP(\s+error!)?(\s*status:?)?\s*\d+(\s*:.*)?$/i.test(detail)) {
    return true
  }
  if (/^failed to [a-z ]+:\s*\d+$/i.test(detail)) return true
  if (/install failed(:\s*\d+)?$/i.test(detail)) return true
  if (/csrf token (unavailable|refresh failed)/i.test(detail)) return true
  if (/^\{[\s\S]*\}$/.test(detail)) return true
  if (/failed to fetch|networkerror|load failed/i.test(detail)) return true
  if (/^unknown error$/i.test(detail)) return true
  if (/^(unauthorized|forbidden|not found|bad request|conflict)$/i.test(detail)) {
    return true
  }
  if (
    /^(user|channel|room|ring|session|transfer|filter|player) not found$/i.test(
      detail,
    )
  ) {
    return true
  }
  if (/^internal (server )?error$/i.test(detail)) return true
  if (/^service unavailable$/i.test(detail)) return true
  if (/^request timeout$/i.test(detail)) return true
  if (/^operation failed$/i.test(detail)) return true
  if (/^failed$/i.test(detail)) return true
  if (/^ai generation failed$/i.test(detail)) return true
  if (/^ai error:/i.test(detail)) return true
  if (
    /^failed to (save|load|get|publish|rotate|compose|process|verify|create|update|set|read|refresh|fetch|parse|start|decode|clear|collect|restore|seal|persist) /i.test(
      detail,
    )
  ) {
    return true
  }
  if (/^no library data available$/i.test(detail)) return true
  if (/^action failed$/i.test(detail)) return true
  if (/^discovery failed$|^import failed$|^failed to add$/i.test(detail)) {
    return true
  }
  if (/^database error$/i.test(detail)) return true
  if (/\((?:HTTP\s*)?\d{3}\)$/i.test(detail)) {
    const inner = detail.replaceAll(/\s*\((?:HTTP\s*)?\d{3}\)\s*$/gi, '').trim()
    if (
      !inner ||
      /^could not [a-z ]+$/i.test(inner) ||
      /^failed to [a-z ]+$/i.test(inner)
    ) {
      return true
    }
  }
  return false
}

export function isInternalDump(text: string): boolean {
  const detail = text.replaceAll(/\s+/g, ' ').trim()
  if (!detail) return true
  if (
    /relation "|does not exist|duplicate key value|violates (unique|not-null|foreign)/i.test(
      detail,
    )
  ) {
    return true
  }
  if (
    /missing field|at line \d+|expected value|key must be a string|eof while parsing|trailing characters|invalid length/i.test(
      detail,
    )
  ) {
    return true
  }
  if (
    /error sending request|os error \d+|builder error|error trying to connect/i.test(
      detail,
    )
  ) {
    return true
  }
  if (/zip (local )?header|invalid zip/i.test(detail)) return true
  if (/^\{[\s\S]*\}$/.test(detail) || /<html[\s>]|<\/html>/i.test(detail)) {
    return true
  }
  if (/RequestTokenError|invalid_grant|invalid_client/i.test(detail)) return true
  return false
}
