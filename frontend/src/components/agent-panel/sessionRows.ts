/**
 * Rows made in a turn that started a new session have no session id yet: the server names
 * the session only once the turn is under way. They belong to it, and until they say so a
 * confirmation in them is outside every session and cannot be answered.
 */
export function adoptSessionRows<T extends { sessionId?: string }>(rows: T[], sessionId: string): T[] {
  return rows.some((row) => !row.sessionId)
    ? rows.map((row) => (row.sessionId ? row : { ...row, sessionId }))
    : rows
}
