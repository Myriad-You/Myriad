let navigationStack: string[] = ['/']

/** Stack for transition direction. Back = path equals stack[-2]. Cap 10. */
export function recordNavigation(path: string) {
  const lastPath = navigationStack.at(-1)

  if (
    navigationStack.length > 1 &&
    navigationStack.at(-2) === path
  ) {
    navigationStack.pop()
    return
  }

  if (lastPath !== path) {
    navigationStack.push(path)

    if (navigationStack.length > 10) {
      navigationStack = navigationStack.slice(-10)
    }
  }
}
