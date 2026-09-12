import React, { createContext, useContext } from 'react'

export interface SettingsPageActionsContextValue {
  resetCurrentPage?: () => void | Promise<void>
  canResetCurrentPage?: boolean
  onMobileBack?: () => void
}

const SettingsPageActionsContext =
  createContext<SettingsPageActionsContextValue | null>(null)

export function useSettingsPageActions(): SettingsPageActionsContextValue | null {
  return useContext(SettingsPageActionsContext)
}

export const SettingsPageActionsProvider: React.FC<{
  value: SettingsPageActionsContextValue
  children: React.ReactNode
}> = ({ value, children }) => (
  <SettingsPageActionsContext.Provider value={value}>
    {children}
  </SettingsPageActionsContext.Provider>
)
