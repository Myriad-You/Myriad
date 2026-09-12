import { useEffect } from 'react'

export function useBrewNavExpand(setExpanded: (open: boolean) => void) {
  useEffect(() => {
    const onExpand = (event: Event) => {
      const path = (event as CustomEvent<{ path: string }>).detail?.path
      if (path === '/brew') setExpanded(true)
    }
    window.addEventListener('nav-expand-secondary', onExpand)
    return () => {
      window.removeEventListener('nav-expand-secondary', onExpand)
    }
  }, [setExpanded])
}
