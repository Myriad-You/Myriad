export type PageAnimationStyle = 'normal' | 'fixed'

export type PageAnimationVariant = 'page' | 'fixed' | 'detail'

export interface PageRouteAnimationMeta {
  key: string
  style: PageAnimationStyle
  variant: PageAnimationVariant
  skipScroll: boolean
}

export function resolvePageRouteAnimation(
  pathname: string,
): PageRouteAnimationMeta {
  if (pathname.startsWith('/brew')) {
    return {
      key: '/brew',
      style: 'normal',
      variant: 'page',
      skipScroll: false,
    }
  }

  if (pathname.startsWith('/tapp/run')) {
    return {
      key: pathname,
      style: 'fixed',
      variant: 'fixed',
      skipScroll: true,
    }
  }
  if (pathname === '/tapp/store') {
    return {
      key: pathname,
      style: 'fixed',
      variant: 'fixed',
      skipScroll: true,
    }
  }

  if (pathname.startsWith('/tapp/detail')) {
    return {
      key: pathname,
      style: 'fixed',
      variant: 'detail',
      skipScroll: true,
    }
  }

  if (pathname === '/tapp') {
    return {
      key: '/tapp',
      style: 'normal',
      variant: 'page',
      skipScroll: false,
    }
  }
  if (pathname === '/tapp/playground') {
    return {
      key: pathname,
      style: 'normal',
      variant: 'page',
      skipScroll: false,
    }
  }
  if (pathname.startsWith('/tapp/')) {
    return {
      key: pathname,
      style: 'normal',
      variant: 'page',
      skipScroll: false,
    }
  }

  return {
    key: pathname,
    style: 'normal',
    variant: 'page',
    skipScroll: false,
  }
}
