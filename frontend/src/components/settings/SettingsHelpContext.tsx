import React, { createContext, useContext } from 'react'

export interface SettingsHelpContextValue {
  showDetails: boolean
  setShowDetails: (value: boolean) => void
}

const SettingsHelpContext = createContext<SettingsHelpContextValue | null>(
  null,
)

export function useSettingsHelp(): SettingsHelpContextValue | null {
  return useContext(SettingsHelpContext)
}

export const SettingsHelpProvider: React.FC<{
  value: SettingsHelpContextValue
  children: React.ReactNode
}> = ({ value, children }) => (
  <SettingsHelpContext.Provider value={value}>
    {children}
  </SettingsHelpContext.Provider>
)
