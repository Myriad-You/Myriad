const TAILWIND_MAP: Record<string, string> = {
  hidden: 'display:none',
  block: 'display:block',
  'inline-block': 'display:inline-block',
  inline: 'display:inline',
  flex: 'display:flex',
  'inline-flex': 'display:inline-flex',
  grid: 'display:grid',

  'flex-row': 'flex-direction:row',
  'flex-row-reverse': 'flex-direction:row-reverse',
  'flex-col': 'flex-direction:column',
  'flex-col-reverse': 'flex-direction:column-reverse',

  'flex-wrap': 'flex-wrap:wrap',
  'flex-nowrap': 'flex-wrap:nowrap',
  'flex-wrap-reverse': 'flex-wrap:wrap-reverse',

  'flex-1': 'flex:1 1 0%',
  'flex-auto': 'flex:1 1 auto',
  'flex-initial': 'flex:0 1 auto',
  'flex-none': 'flex:none',
  'flex-shrink-0': 'flex-shrink:0',
  'shrink-0': 'flex-shrink:0',
  'flex-shrink': 'flex-shrink:1',
  shrink: 'flex-shrink:1',
  'flex-grow-0': 'flex-grow:0',
  'grow-0': 'flex-grow:0',
  'flex-grow': 'flex-grow:1',
  grow: 'flex-grow:1',

  'justify-start': 'justify-content:flex-start',
  'justify-end': 'justify-content:flex-end',
  'justify-center': 'justify-content:center',
  'justify-between': 'justify-content:space-between',
  'justify-around': 'justify-content:space-around',
  'justify-evenly': 'justify-content:space-evenly',

  'items-start': 'align-items:flex-start',
  'items-end': 'align-items:flex-end',
  'items-center': 'align-items:center',
  'items-baseline': 'align-items:baseline',
  'items-stretch': 'align-items:stretch',

  'self-auto': 'align-self:auto',
  'self-start': 'align-self:flex-start',
  'self-end': 'align-self:flex-end',
  'self-center': 'align-self:center',
  'self-stretch': 'align-self:stretch',

  static: 'position:static',
  fixed: 'position:fixed',
  absolute: 'position:absolute',
  relative: 'position:relative',
  sticky: 'position:sticky',

  'inset-0': 'top:0;right:0;bottom:0;left:0',
  'inset-x-0': 'left:0;right:0',
  'inset-y-0': 'top:0;bottom:0',
  'top-0': 'top:0',
  'top-1': 'top:0.25rem',
  'top-2': 'top:0.5rem',
  'top-3': 'top:0.75rem',
  'top-4': 'top:1rem',
  'right-0': 'right:0',
  'right-1': 'right:0.25rem',
  'right-2': 'right:0.5rem',
  'right-3': 'right:0.75rem',
  'right-4': 'right:1rem',
  'bottom-0': 'bottom:0',
  'bottom-1': 'bottom:0.25rem',
  'bottom-2': 'bottom:0.5rem',
  'bottom-3': 'bottom:0.75rem',
  'bottom-4': 'bottom:1rem',
  'left-0': 'left:0',
  'left-1': 'left:0.25rem',
  'left-2': 'left:0.5rem',
  'left-3': 'left:0.75rem',
  'left-4': 'left:1rem',
  'left-1/2': 'left:50%',
  'left-1/3': 'left:33.333333%',
  'left-2/3': 'left:66.666667%',
  'left-1/4': 'left:25%',
  'left-3/4': 'left:75%',
  'left-full': 'left:100%',
  '-translate-x-1/2': 'transform:translateX(-50%)',
  '-translate-y-1/2': 'transform:translateY(-50%)',
  'translate-x-1/2': 'transform:translateX(50%)',
  'translate-y-1/2': 'transform:translateY(50%)',
  '-top-2': 'top:-0.5rem',
  '-top-4': 'top:-1rem',
  '-top-6': 'top:-1.5rem',
  '-right-2': 'right:-0.5rem',
  '-right-4': 'right:-1rem',
  '-right-6': 'right:-1.5rem',
  '-bottom-2': 'bottom:-0.5rem',
  '-left-2': 'left:-0.5rem',

  'z-0': 'z-index:0',
  'z-10': 'z-index:10',
  'z-20': 'z-index:20',
  'z-30': 'z-index:30',
  'z-40': 'z-index:40',
  'z-50': 'z-index:50',

  'w-0': 'width:0',
  'w-px': 'width:1px',
  'w-0.5': 'width:0.125rem',
  'w-1': 'width:0.25rem',
  'w-1.5': 'width:0.375rem',
  'w-2': 'width:0.5rem',
  'w-2.5': 'width:0.625rem',
  'w-3': 'width:0.75rem',
  'w-3.5': 'width:0.875rem',
  'w-4': 'width:1rem',
  'w-5': 'width:1.25rem',
  'w-6': 'width:1.5rem',
  'w-7': 'width:1.75rem',
  'w-8': 'width:2rem',
  'w-9': 'width:2.25rem',
  'w-10': 'width:2.5rem',
  'w-11': 'width:2.75rem',
  'w-12': 'width:3rem',
  'w-14': 'width:3.5rem',
  'w-16': 'width:4rem',
  'w-20': 'width:5rem',
  'w-24': 'width:6rem',
  'w-28': 'width:7rem',
  'w-32': 'width:8rem',
  'w-36': 'width:9rem',
  'w-40': 'width:10rem',
  'w-44': 'width:11rem',
  'w-48': 'width:12rem',
  'w-52': 'width:13rem',
  'w-56': 'width:14rem',
  'w-60': 'width:15rem',
  'w-64': 'width:16rem',
  'w-72': 'width:18rem',
  'w-80': 'width:20rem',
  'w-96': 'width:24rem',
  'w-auto': 'width:auto',
  'w-full': 'width:100%',
  'w-screen': 'width:100vw',
  'w-min': 'width:min-content',
  'w-max': 'width:max-content',
  'w-fit': 'width:fit-content',
  'min-w-0': 'min-width:0',
  'min-w-full': 'min-width:100%',
  'max-w-none': 'max-width:none',
  'max-w-xs': 'max-width:20rem',
  'max-w-sm': 'max-width:24rem',
  'max-w-md': 'max-width:28rem',
  'max-w-lg': 'max-width:32rem',
  'max-w-xl': 'max-width:36rem',
  'max-w-2xl': 'max-width:42rem',
  'max-w-3xl': 'max-width:48rem',
  'max-w-4xl': 'max-width:56rem',
  'max-w-5xl': 'max-width:64rem',
  'max-w-6xl': 'max-width:72rem',
  'max-w-7xl': 'max-width:80rem',
  'max-w-full': 'max-width:100%',

  'h-0': 'height:0',
  'h-px': 'height:1px',
  'h-0.5': 'height:0.125rem',
  'h-1': 'height:0.25rem',
  'h-1.5': 'height:0.375rem',
  'h-2': 'height:0.5rem',
  'h-2.5': 'height:0.625rem',
  'h-3': 'height:0.75rem',
  'h-3.5': 'height:0.875rem',
  'h-4': 'height:1rem',
  'h-5': 'height:1.25rem',
  'h-6': 'height:1.5rem',
  'h-7': 'height:1.75rem',
  'h-8': 'height:2rem',
  'h-9': 'height:2.25rem',
  'h-10': 'height:2.5rem',
  'h-11': 'height:2.75rem',
  'h-12': 'height:3rem',
  'h-14': 'height:3.5rem',
  'h-16': 'height:4rem',
  'h-20': 'height:5rem',
  'h-24': 'height:6rem',
  'h-28': 'height:7rem',
  'h-32': 'height:8rem',
  'h-36': 'height:9rem',
  'h-40': 'height:10rem',
  'h-44': 'height:11rem',
  'h-48': 'height:12rem',
  'h-52': 'height:13rem',
  'h-56': 'height:14rem',
  'h-60': 'height:15rem',
  'h-64': 'height:16rem',
  'h-72': 'height:18rem',
  'h-80': 'height:20rem',
  'h-96': 'height:24rem',
  'h-auto': 'height:auto',
  'h-full': 'height:100%',
  'h-screen': 'height:100vh',
  'h-min': 'height:min-content',
  'h-max': 'height:max-content',
  'h-fit': 'height:fit-content',
  'min-h-0': 'min-height:0',
  'min-h-full': 'min-height:100%',
  'min-h-screen': 'min-height:100vh',
  'max-h-0': 'max-height:0',
  'max-h-1': 'max-height:0.25rem',
  'max-h-2': 'max-height:0.5rem',
  'max-h-3': 'max-height:0.75rem',
  'max-h-4': 'max-height:1rem',
  'max-h-5': 'max-height:1.25rem',
  'max-h-6': 'max-height:1.5rem',
  'max-h-8': 'max-height:2rem',
  'max-h-10': 'max-height:2.5rem',
  'max-h-12': 'max-height:3rem',
  'max-h-16': 'max-height:4rem',
  'max-h-20': 'max-height:5rem',
  'max-h-24': 'max-height:6rem',
  'max-h-32': 'max-height:8rem',
  'max-h-40': 'max-height:10rem',
  'max-h-48': 'max-height:12rem',
  'max-h-56': 'max-height:14rem',
  'max-h-64': 'max-height:16rem',
  'max-h-72': 'max-height:18rem',
  'max-h-80': 'max-height:20rem',
  'max-h-96': 'max-height:24rem',
  'max-h-full': 'max-height:100%',
  'max-h-screen': 'max-height:100vh',

  'p-0': 'padding:0',
  'p-0.5': 'padding:0.125rem',
  'p-1': 'padding:0.25rem',
  'p-1.5': 'padding:0.375rem',
  'p-2': 'padding:0.5rem',
  'p-2.5': 'padding:0.625rem',
  'p-3': 'padding:0.75rem',
  'p-3.5': 'padding:0.875rem',
  'p-4': 'padding:1rem',
  'p-5': 'padding:1.25rem',
  'p-6': 'padding:1.5rem',
  'p-7': 'padding:1.75rem',
  'p-8': 'padding:2rem',
  'p-9': 'padding:2.25rem',
  'p-10': 'padding:2.5rem',
  'p-11': 'padding:2.75rem',
  'p-12': 'padding:3rem',
  'px-0': 'padding-left:0;padding-right:0',
  'px-0.5': 'padding-left:0.125rem;padding-right:0.125rem',
  'px-1': 'padding-left:0.25rem;padding-right:0.25rem',
  'px-1.5': 'padding-left:0.375rem;padding-right:0.375rem',
  'px-2': 'padding-left:0.5rem;padding-right:0.5rem',
  'px-2.5': 'padding-left:0.625rem;padding-right:0.625rem',
  'px-3': 'padding-left:0.75rem;padding-right:0.75rem',
  'px-3.5': 'padding-left:0.875rem;padding-right:0.875rem',
  'px-4': 'padding-left:1rem;padding-right:1rem',
  'px-5': 'padding-left:1.25rem;padding-right:1.25rem',
  'px-6': 'padding-left:1.5rem;padding-right:1.5rem',
  'px-8': 'padding-left:2rem;padding-right:2rem',
  'py-0': 'padding-top:0;padding-bottom:0',
  'py-0.5': 'padding-top:0.125rem;padding-bottom:0.125rem',
  'py-1': 'padding-top:0.25rem;padding-bottom:0.25rem',
  'py-1.5': 'padding-top:0.375rem;padding-bottom:0.375rem',
  'py-2': 'padding-top:0.5rem;padding-bottom:0.5rem',
  'py-2.5': 'padding-top:0.625rem;padding-bottom:0.625rem',
  'py-3': 'padding-top:0.75rem;padding-bottom:0.75rem',
  'py-3.5': 'padding-top:0.875rem;padding-bottom:0.875rem',
  'py-4': 'padding-top:1rem;padding-bottom:1rem',
  'py-5': 'padding-top:1.25rem;padding-bottom:1.25rem',
  'py-6': 'padding-top:1.5rem;padding-bottom:1.5rem',
  'py-8': 'padding-top:2rem;padding-bottom:2rem',
  'pt-0': 'padding-top:0',
  'pt-1': 'padding-top:0.25rem',
  'pt-2': 'padding-top:0.5rem',
  'pt-3': 'padding-top:0.75rem',
  'pt-4': 'padding-top:1rem',
  'pt-5': 'padding-top:1.25rem',
  'pt-6': 'padding-top:1.5rem',
  'pt-8': 'padding-top:2rem',
  'pt-10': 'padding-top:2.5rem',
  'pt-12': 'padding-top:3rem',
  'pt-16': 'padding-top:4rem',
  'pt-20': 'padding-top:5rem',
  'pt-24': 'padding-top:6rem',
  'pr-0': 'padding-right:0',
  'pr-1': 'padding-right:0.25rem',
  'pr-2': 'padding-right:0.5rem',
  'pr-3': 'padding-right:0.75rem',
  'pr-4': 'padding-right:1rem',
  'pb-0': 'padding-bottom:0',
  'pb-1': 'padding-bottom:0.25rem',
  'pb-2': 'padding-bottom:0.5rem',
  'pb-3': 'padding-bottom:0.75rem',
  'pb-4': 'padding-bottom:1rem',
  'pb-6': 'padding-bottom:1.5rem',
  'pb-8': 'padding-bottom:2rem',
  'pb-10': 'padding-bottom:2.5rem',
  'pb-12': 'padding-bottom:3rem',
  'pb-16': 'padding-bottom:4rem',
  'pb-20': 'padding-bottom:5rem',
  'pb-24': 'padding-bottom:6rem',
  'pl-0': 'padding-left:0',
  'pl-1': 'padding-left:0.25rem',
  'pl-2': 'padding-left:0.5rem',
  'pl-3': 'padding-left:0.75rem',
  'pl-4': 'padding-left:1rem',

  'm-0': 'margin:0',
  'm-0.5': 'margin:0.125rem',
  'm-1': 'margin:0.25rem',
  'm-1.5': 'margin:0.375rem',
  'm-2': 'margin:0.5rem',
  'm-2.5': 'margin:0.625rem',
  'm-3': 'margin:0.75rem',
  'm-4': 'margin:1rem',
  'm-5': 'margin:1.25rem',
  'm-6': 'margin:1.5rem',
  'm-8': 'margin:2rem',
  'm-auto': 'margin:auto',
  'mx-0': 'margin-left:0;margin-right:0',
  'mx-1': 'margin-left:0.25rem;margin-right:0.25rem',
  'mx-2': 'margin-left:0.5rem;margin-right:0.5rem',
  'mx-3': 'margin-left:0.75rem;margin-right:0.75rem',
  'mx-4': 'margin-left:1rem;margin-right:1rem',
  'mx-auto': 'margin-left:auto;margin-right:auto',
  'my-0': 'margin-top:0;margin-bottom:0',
  'my-1': 'margin-top:0.25rem;margin-bottom:0.25rem',
  'my-2': 'margin-top:0.5rem;margin-bottom:0.5rem',
  'my-3': 'margin-top:0.75rem;margin-bottom:0.75rem',
  'my-4': 'margin-top:1rem;margin-bottom:1rem',
  'my-auto': 'margin-top:auto;margin-bottom:auto',
  'mt-0': 'margin-top:0',
  'mt-0.5': 'margin-top:0.125rem',
  'mt-1': 'margin-top:0.25rem',
  'mt-1.5': 'margin-top:0.375rem',
  'mt-2': 'margin-top:0.5rem',
  'mt-3': 'margin-top:0.75rem',
  'mt-4': 'margin-top:1rem',
  'mt-5': 'margin-top:1.25rem',
  'mt-6': 'margin-top:1.5rem',
  'mt-8': 'margin-top:2rem',
  'mt-auto': 'margin-top:auto',
  'mr-0': 'margin-right:0',
  'mr-1': 'margin-right:0.25rem',
  'mr-2': 'margin-right:0.5rem',
  'mr-3': 'margin-right:0.75rem',
  'mr-4': 'margin-right:1rem',
  'mr-auto': 'margin-right:auto',
  'mb-0': 'margin-bottom:0',
  'mb-0.5': 'margin-bottom:0.125rem',
  'mb-1': 'margin-bottom:0.25rem',
  'mb-1.5': 'margin-bottom:0.375rem',
  'mb-2': 'margin-bottom:0.5rem',
  'mb-3': 'margin-bottom:0.75rem',
  'mb-4': 'margin-bottom:1rem',
  'mb-5': 'margin-bottom:1.25rem',
  'mb-6': 'margin-bottom:1.5rem',
  'mb-8': 'margin-bottom:2rem',
  'ml-0': 'margin-left:0',
  'ml-1': 'margin-left:0.25rem',
  'ml-2': 'margin-left:0.5rem',
  'ml-3': 'margin-left:0.75rem',
  'ml-4': 'margin-left:1rem',
  'ml-auto': 'margin-left:auto',
  '-mt-1': 'margin-top:-0.25rem',
  '-mt-2': 'margin-top:-0.5rem',
  '-mb-1': 'margin-bottom:-0.25rem',
  '-ml-1': 'margin-left:-0.25rem',
  '-mr-1': 'margin-right:-0.25rem',

  'gap-0': 'gap:0',
  'gap-0.5': 'gap:0.125rem',
  'gap-1': 'gap:0.25rem',
  'gap-1.5': 'gap:0.375rem',
  'gap-2': 'gap:0.5rem',
  'gap-2.5': 'gap:0.625rem',
  'gap-3': 'gap:0.75rem',
  'gap-3.5': 'gap:0.875rem',
  'gap-4': 'gap:1rem',
  'gap-5': 'gap:1.25rem',
  'gap-6': 'gap:1.5rem',
  'gap-8': 'gap:2rem',
  'gap-10': 'gap:2.5rem',
  'gap-12': 'gap:3rem',

  'rounded-none': 'border-radius:0',
  'rounded-sm': 'border-radius:0.125rem',
  rounded: 'border-radius:0.25rem',
  'rounded-md': 'border-radius:0.375rem',
  'rounded-lg': 'border-radius:0.5rem',
  'rounded-xl': 'border-radius:0.75rem',
  'rounded-2xl': 'border-radius:1rem',
  'rounded-3xl': 'border-radius:1.5rem',
  'rounded-full': 'border-radius:9999px',

  border: 'border-width:1px',
  'border-0': 'border-width:0',
  'border-2': 'border-width:2px',
  'border-4': 'border-width:4px',
  'border-8': 'border-width:8px',
  'border-t': 'border-top-width:1px',
  'border-r': 'border-right-width:1px',
  'border-b': 'border-bottom-width:1px',
  'border-l': 'border-left-width:1px',

  'border-solid': 'border-style:solid',
  'border-dashed': 'border-style:dashed',
  'border-dotted': 'border-style:dotted',
  'border-none': 'border-style:none',

  'border-transparent': 'border-color:transparent',
  'border-white': 'border-color:#fff',
  'border-black': 'border-color:#000',
  'border-current': 'border-color:currentColor',
  'border-neutral-200': 'border-color:rgb(229 229 229)',
  'border-neutral-300': 'border-color:rgb(212 212 212)',
  'border-neutral-400': 'border-color:rgb(163 163 163)',
  'border-neutral-500': 'border-color:rgb(115 115 115)',
  'border-neutral-600': 'border-color:rgb(82 82 82)',
  'border-neutral-700': 'border-color:rgb(64 64 64)',
  'border-neutral-800': 'border-color:rgb(38 38 38)',

  'bg-transparent': 'background-color:transparent',
  'bg-current': 'background-color:currentColor',
  'bg-white': 'background-color:#fff',
  'bg-black': 'background-color:#000',

  'text-transparent': 'color:transparent',
  'text-white': 'color:#fff',
  'text-black': 'color:#000',

  'text-xs': 'font-size:0.75rem;line-height:1rem',
  'text-sm': 'font-size:0.875rem;line-height:1.25rem',
  'text-base': 'font-size:1rem;line-height:1.5rem',
  'text-lg': 'font-size:1.125rem;line-height:1.75rem',
  'text-xl': 'font-size:1.25rem;line-height:1.75rem',
  'text-2xl': 'font-size:1.5rem;line-height:2rem',
  'text-3xl': 'font-size:1.875rem;line-height:2.25rem',
  'text-4xl': 'font-size:2.25rem;line-height:2.5rem',
  'text-5xl': 'font-size:3rem;line-height:1',

  'font-thin': 'font-weight:100',
  'font-extralight': 'font-weight:200',
  'font-light': 'font-weight:300',
  'font-normal': 'font-weight:400',
  'font-medium': 'font-weight:500',
  'font-semibold': 'font-weight:600',
  'font-bold': 'font-weight:700',
  'font-extrabold': 'font-weight:800',
  'font-black': 'font-weight:900',

  'font-sans':
    'font-family:ui-sans-serif,system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,"Helvetica Neue",Arial,sans-serif',
  'font-serif':
    'font-family:ui-serif,Georgia,Cambria,"Times New Roman",Times,serif',
  'font-mono':
    'font-family:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,"Liberation Mono","Courier New",monospace',

  'text-left': 'text-align:left',
  'text-center': 'text-align:center',
  'text-right': 'text-align:right',
  'text-justify': 'text-align:justify',

  truncate: 'overflow:hidden;text-overflow:ellipsis;white-space:nowrap',
  'overflow-ellipsis': 'text-overflow:ellipsis',
  'overflow-clip': 'text-overflow:clip',

  'whitespace-normal': 'white-space:normal',
  'whitespace-nowrap': 'white-space:nowrap',
  'whitespace-pre': 'white-space:pre',
  'whitespace-pre-line': 'white-space:pre-line',
  'whitespace-pre-wrap': 'white-space:pre-wrap',

  'break-normal': 'overflow-wrap:normal;word-break:normal',
  'break-words': 'overflow-wrap:break-word',
  'wrap-break-word': 'overflow-wrap:break-word',
  'break-all': 'word-break:break-all',

  'line-clamp-1':
    'overflow:hidden;display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:1',
  'line-clamp-2':
    'overflow:hidden;display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:2',
  'line-clamp-3':
    'overflow:hidden;display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:3',

  'opacity-0': 'opacity:0',
  'opacity-5': 'opacity:0.05',
  'opacity-10': 'opacity:0.1',
  'opacity-20': 'opacity:0.2',
  'opacity-25': 'opacity:0.25',
  'opacity-30': 'opacity:0.3',
  'opacity-40': 'opacity:0.4',
  'opacity-50': 'opacity:0.5',
  'opacity-60': 'opacity:0.6',
  'opacity-70': 'opacity:0.7',
  'opacity-75': 'opacity:0.75',
  'opacity-80': 'opacity:0.8',
  'opacity-90': 'opacity:0.9',
  'opacity-95': 'opacity:0.95',
  'opacity-100': 'opacity:1',

  'overflow-auto': 'overflow:auto',
  'overflow-hidden': 'overflow:hidden',
  'overflow-visible': 'overflow:visible',
  'overflow-scroll': 'overflow:scroll',
  'overflow-x-auto': 'overflow-x:auto',
  'overflow-y-auto': 'overflow-y:auto',
  'overflow-x-hidden': 'overflow-x:hidden',
  'overflow-y-hidden': 'overflow-y:hidden',

  visible: 'visibility:visible',
  invisible: 'visibility:hidden',

  'cursor-auto': 'cursor:auto',
  'cursor-default': 'cursor:default',
  'cursor-pointer': 'cursor:pointer',
  'cursor-wait': 'cursor:wait',
  'cursor-text': 'cursor:text',
  'cursor-move': 'cursor:move',
  'cursor-not-allowed': 'cursor:not-allowed',

  'pointer-events-none': 'pointer-events:none',
  'pointer-events-auto': 'pointer-events:auto',

  'select-none': 'user-select:none',
  'select-text': 'user-select:text',
  'select-all': 'user-select:all',
  'select-auto': 'user-select:auto',

  'outline-none': 'outline:2px solid transparent;outline-offset:2px',
  outline: 'outline-style:solid',

  'resize-none': 'resize:none',
  'resize-y': 'resize:vertical',
  'resize-x': 'resize:horizontal',
  resize: 'resize:both',

  'shadow-sm': 'box-shadow:0 1px 2px 0 rgb(0 0 0/0.05)',
  shadow: 'box-shadow:0 1px 3px 0 rgb(0 0 0/0.1),0 1px 2px -1px rgb(0 0 0/0.1)',
  'shadow-md':
    'box-shadow:0 4px 6px -1px rgb(0 0 0/0.1),0 2px 4px -2px rgb(0 0 0/0.1)',
  'shadow-lg':
    'box-shadow:0 10px 15px -3px rgb(0 0 0/0.1),0 4px 6px -4px rgb(0 0 0/0.1)',
  'shadow-xl':
    'box-shadow:0 20px 25px -5px rgb(0 0 0/0.1),0 8px 10px -6px rgb(0 0 0/0.1)',
  'shadow-2xl': 'box-shadow:0 25px 50px -12px rgb(0 0 0/0.25)',
  'shadow-none': 'box-shadow:none',

  'transition-none': 'transition-property:none',
  'transition-all':
    'transition-property:all;transition-timing-function:cubic-bezier(0.4,0,0.2,1);transition-duration:150ms',
  transition:
    'transition-property:color,background-color,border-color,text-decoration-color,fill,stroke,opacity,box-shadow,transform,filter,backdrop-filter;transition-timing-function:cubic-bezier(0.4,0,0.2,1);transition-duration:150ms',
  'transition-colors':
    'transition-property:color,background-color,border-color,text-decoration-color,fill,stroke;transition-timing-function:cubic-bezier(0.4,0,0.2,1);transition-duration:150ms',
  'transition-opacity':
    'transition-property:opacity;transition-timing-function:cubic-bezier(0.4,0,0.2,1);transition-duration:150ms',
  'transition-transform':
    'transition-property:transform;transition-timing-function:cubic-bezier(0.4,0,0.2,1);transition-duration:150ms',

  'duration-75': 'transition-duration:75ms',
  'duration-100': 'transition-duration:100ms',
  'duration-150': 'transition-duration:150ms',
  'duration-200': 'transition-duration:200ms',
  'duration-300': 'transition-duration:300ms',
  'duration-400': 'transition-duration:400ms',
  'duration-500': 'transition-duration:500ms',
  'duration-700': 'transition-duration:700ms',
  'duration-1000': 'transition-duration:1000ms',

  transform:
    'transform:translate(var(--tw-translate-x,0),var(--tw-translate-y,0)) rotate(var(--tw-rotate,0)) skewX(var(--tw-skew-x,0)) skewY(var(--tw-skew-y,0)) scaleX(var(--tw-scale-x,1)) scaleY(var(--tw-scale-y,1))',
  'transform-gpu':
    'transform:translate3d(var(--tw-translate-x,0),var(--tw-translate-y,0),0) rotate(var(--tw-rotate,0)) skewX(var(--tw-skew-x,0)) skewY(var(--tw-skew-y,0)) scaleX(var(--tw-scale-x,1)) scaleY(var(--tw-scale-y,1))',
  'transform-none': 'transform:none',
  'translate-x-0': 'transform:translateX(0)',
  'translate-x-1': 'transform:translateX(0.25rem)',
  'translate-x-2': 'transform:translateX(0.5rem)',
  'translate-x-4': 'transform:translateX(1rem)',
  '-translate-x-1': 'transform:translateX(-0.25rem)',
  '-translate-x-2': 'transform:translateX(-0.5rem)',
  '-translate-x-4': 'transform:translateX(-1rem)',
  'translate-y-0': 'transform:translateY(0)',
  'translate-y-1': 'transform:translateY(0.25rem)',
  'translate-y-2': 'transform:translateY(0.5rem)',
  'translate-y-4': 'transform:translateY(1rem)',
  '-translate-y-1': 'transform:translateY(-0.25rem)',
  '-translate-y-2': 'transform:translateY(-0.5rem)',
  '-translate-y-4': 'transform:translateY(-1rem)',
  'scale-0': 'transform:scale(0)',
  'scale-50': 'transform:scale(.5)',
  'scale-75': 'transform:scale(.75)',
  'scale-90': 'transform:scale(.9)',
  'scale-95': 'transform:scale(.95)',
  'scale-100': 'transform:scale(1)',
  'scale-105': 'transform:scale(1.05)',
  'scale-110': 'transform:scale(1.1)',
  'scale-125': 'transform:scale(1.25)',
  'scale-150': 'transform:scale(1.5)',

  'backdrop-blur-none': 'backdrop-filter:blur(0)',
  'backdrop-blur-sm': 'backdrop-filter:blur(3.4px)',
  'backdrop-blur': 'backdrop-filter:blur(6.8px)',
  'backdrop-blur-md': 'backdrop-filter:blur(10.2px)',
  'backdrop-blur-lg': 'backdrop-filter:blur(13.6px)',
  'backdrop-blur-xl': 'backdrop-filter:blur(20.4px)',
  'backdrop-blur-2xl': 'backdrop-filter:blur(34px)',
  'backdrop-blur-3xl': 'backdrop-filter:blur(54.4px)',

  'object-contain': 'object-fit:contain',
  'object-cover': 'object-fit:cover',
  'object-fill': 'object-fit:fill',
  'object-none': 'object-fit:none',
  'object-scale-down': 'object-fit:scale-down',

  'aspect-auto': 'aspect-ratio:auto',
  'aspect-square': 'aspect-ratio:1/1',
  'aspect-video': 'aspect-ratio:16/9',

  'space-x-0': '--tw-space-x-reverse:0',
  'space-x-1': '--tw-space-x-reverse:0',
  'space-x-2': '--tw-space-x-reverse:0',
  'space-x-3': '--tw-space-x-reverse:0',
  'space-x-4': '--tw-space-x-reverse:0',
  'space-y-0': '--tw-space-y-reverse:0',
  'space-y-1': '--tw-space-y-reverse:0',
  'space-y-2': '--tw-space-y-reverse:0',
  'space-y-3': '--tw-space-y-reverse:0',
  'space-y-4': '--tw-space-y-reverse:0',

  'grid-cols-1': 'grid-template-columns:repeat(1,minmax(0,1fr))',
  'grid-cols-2': 'grid-template-columns:repeat(2,minmax(0,1fr))',
  'grid-cols-3': 'grid-template-columns:repeat(3,minmax(0,1fr))',
  'grid-cols-4': 'grid-template-columns:repeat(4,minmax(0,1fr))',
  'grid-cols-5': 'grid-template-columns:repeat(5,minmax(0,1fr))',
  'grid-cols-6': 'grid-template-columns:repeat(6,minmax(0,1fr))',
  'grid-rows-1': 'grid-template-rows:repeat(1,minmax(0,1fr))',
  'grid-rows-2': 'grid-template-rows:repeat(2,minmax(0,1fr))',
  'grid-rows-3': 'grid-template-rows:repeat(3,minmax(0,1fr))',
  'grid-rows-4': 'grid-template-rows:repeat(4,minmax(0,1fr))',
  'col-span-1': 'grid-column:span 1/span 1',
  'col-span-2': 'grid-column:span 2/span 2',
  'col-span-3': 'grid-column:span 3/span 3',
  'col-span-4': 'grid-column:span 4/span 4',
  'col-span-full': 'grid-column:1/-1',
  'row-span-1': 'grid-row:span 1/span 1',
  'row-span-2': 'grid-row:span 2/span 2',
  'row-span-3': 'grid-row:span 3/span 3',
  'row-span-full': 'grid-row:1/-1',

  'min-h-min': 'min-height:min-content',
  'min-h-max': 'min-height:max-content',
  'min-h-fit': 'min-height:fit-content',

  'leading-none': 'line-height:1',
  'leading-tight': 'line-height:1.25',
  'leading-snug': 'line-height:1.375',
  'leading-normal': 'line-height:1.5',
  'leading-relaxed': 'line-height:1.625',
  'leading-loose': 'line-height:2',

  'animate-none': 'animation:none',
  'animate-spin': 'animation:spin 1s linear infinite',
  'animate-ping': 'animation:ping 1s cubic-bezier(0,0,0.2,1) infinite',
  'animate-pulse': 'animation:pulse 2s cubic-bezier(0.4,0,0.6,1) infinite',
  'animate-bounce': 'animation:bounce 1s infinite',
}

function extractClassNames(source: string): Set<string> {
  const classes = new Set<string>()

  const addClasses = (classString: string) => {
    classString.split(/\s+/).forEach((cls) => {
      const trimmed = cls.trim()
      if (
        trimmed &&
        !trimmed.includes('(') &&
        !trimmed.includes('{') &&
        !trimmed.includes('<') &&
        !trimmed.includes(';') &&
        !trimmed.includes('=') &&
        !trimmed.includes('$') &&
        !trimmed.includes('function') &&
        trimmed.length < 80
      ) {
        classes.add(trimmed)
      }
    })
  }

  const htmlClassRegex = /class=["']([^"']+)["']/g
  let match = htmlClassRegex.exec(source)
  while (match !== null) {
    addClasses(match[1])
    match = htmlClassRegex.exec(source)
  }

  const classNameAssignRegex = /\.className\s*\+?=\s*["'`]([^"'`]+)["'`]/g
  match = classNameAssignRegex.exec(source)
  while (match !== null) {
    addClasses(match[1])
    match = classNameAssignRegex.exec(source)
  }

  // 3. 匹配 JS classList.add('...') / classList.remove('...') / classList.toggle('...')
  const classListRegex =
    /classList\.(add|remove|toggle|contains)\s*\(\s*["'`]([^"'`]+)["'`]/g
  match = classListRegex.exec(source)
  while (match !== null) {
    addClasses(match[2])
    match = classListRegex.exec(source)
  }

  // 4. 匹配三元表达式中的类名字符串
  // 例如: (role === 'user' ? 'flex-row-reverse msg-user-enter' : 'msg-ai-enter')
  const ternaryClassRegex =
    /\?\s*["'`]([^"'`]+)["'`]\s*:\s*["'`]([^"'`]*)["'`]/g
  match = ternaryClassRegex.exec(source)
  while (match !== null) {
    addClasses(match[1])
    addClasses(match[2])
    match = ternaryClassRegex.exec(source)
  }

  const looseClassRegex = /["'`]([-\w:/[\].!]+(?:\s+[-\w:/[\].!]+)*)["'`]/g
  match = looseClassRegex.exec(source)
  while (match !== null) {
    const value = match[1]
    // 只添加看起来像 Tailwind 类的内容
    if (
      /^[-\w:/[\].!\s]+$/.test(value) &&
      !value.includes('http') &&
      !value.includes('://') &&
      value.length < 200
    ) {
      addClasses(value)
    }
    match = looseClassRegex.exec(source)
  }

  return classes
}

const COLORS: Record<string, string> = {
  transparent: 'transparent',
  current: 'currentColor',
  black: '#000',
  white: '#fff',
  'slate-50': '#f8fafc',
  'slate-100': '#f1f5f9',
  'slate-200': '#e2e8f0',
  'slate-300': '#cbd5e1',
  'slate-400': '#94a3b8',
  'slate-500': '#64748b',
  'slate-600': '#475569',
  'slate-700': '#334155',
  'slate-800': '#1e293b',
  'slate-900': '#0f172a',
  'slate-950': '#020617',
  'gray-50': '#f9fafb',
  'gray-100': '#f3f4f6',
  'gray-200': '#e5e7eb',
  'gray-300': '#d1d5db',
  'gray-400': '#9ca3af',
  'gray-500': '#6b7280',
  'gray-600': '#4b5563',
  'gray-700': '#374151',
  'gray-800': '#1f2937',
  'gray-900': '#111827',
  'gray-950': '#030712',
  'zinc-50': '#fafafa',
  'zinc-100': '#f4f4f5',
  'zinc-200': '#e4e4e7',
  'zinc-300': '#d4d4d8',
  'zinc-400': '#a1a1aa',
  'zinc-500': '#71717a',
  'zinc-600': '#52525b',
  'zinc-700': '#3f3f46',
  'zinc-800': '#27272a',
  'zinc-900': '#18181b',
  'zinc-950': '#09090b',
  'neutral-50': '#fafafa',
  'neutral-100': '#f5f5f5',
  'neutral-200': '#e5e5e5',
  'neutral-300': '#d4d4d4',
  'neutral-400': '#a3a3a3',
  'neutral-500': '#737373',
  'neutral-600': '#525252',
  'neutral-700': '#404040',
  'neutral-800': '#262626',
  'neutral-900': '#171717',
  'neutral-950': '#0a0a0a',
  'stone-50': '#fafaf9',
  'stone-100': '#f5f5f4',
  'stone-200': '#e7e5e4',
  'stone-300': '#d6d3d1',
  'stone-400': '#a8a29e',
  'stone-500': '#78716c',
  'stone-600': '#57534e',
  'stone-700': '#44403c',
  'stone-800': '#292524',
  'stone-900': '#1c1917',
  'stone-950': '#0c0a09',
  'red-50': '#fef2f2',
  'red-100': '#fee2e2',
  'red-200': '#fecaca',
  'red-300': '#fca5a5',
  'red-400': '#f87171',
  'red-500': '#ef4444',
  'red-600': '#dc2626',
  'red-700': '#b91c1c',
  'red-800': '#991b1b',
  'red-900': '#7f1d1d',
  'red-950': '#450a0a',
  'orange-50': '#fff7ed',
  'orange-100': '#ffedd5',
  'orange-200': '#fed7aa',
  'orange-300': '#fdba74',
  'orange-400': '#fb923c',
  'orange-500': '#f97316',
  'orange-600': '#ea580c',
  'orange-700': '#c2410c',
  'orange-800': '#9a3412',
  'orange-900': '#7c2d12',
  'orange-950': '#431407',
  'amber-50': '#fffbeb',
  'amber-100': '#fef3c7',
  'amber-200': '#fde68a',
  'amber-300': '#fcd34d',
  'amber-400': '#fbbf24',
  'amber-500': '#f59e0b',
  'amber-600': '#d97706',
  'amber-700': '#b45309',
  'amber-800': '#92400e',
  'amber-900': '#78350f',
  'amber-950': '#451a03',
  'yellow-50': '#fefce8',
  'yellow-100': '#fef9c3',
  'yellow-200': '#fef08a',
  'yellow-300': '#fde047',
  'yellow-400': '#facc15',
  'yellow-500': '#eab308',
  'yellow-600': '#ca8a04',
  'yellow-700': '#a16207',
  'yellow-800': '#854d0e',
  'yellow-900': '#713f12',
  'yellow-950': '#422006',
  'lime-50': '#f7fee7',
  'lime-100': '#ecfccb',
  'lime-200': '#d9f99d',
  'lime-300': '#bef264',
  'lime-400': '#a3e635',
  'lime-500': '#84cc16',
  'lime-600': '#65a30d',
  'lime-700': '#4d7c0f',
  'lime-800': '#3f6212',
  'lime-900': '#365314',
  'lime-950': '#1a2e05',
  'green-50': '#f0fdf4',
  'green-100': '#dcfce7',
  'green-200': '#bbf7d0',
  'green-300': '#86efac',
  'green-400': '#4ade80',
  'green-500': '#22c55e',
  'green-600': '#16a34a',
  'green-700': '#15803d',
  'green-800': '#166534',
  'green-900': '#14532d',
  'green-950': '#052e16',
  'emerald-50': '#ecfdf5',
  'emerald-100': '#d1fae5',
  'emerald-200': '#a7f3d0',
  'emerald-300': '#6ee7b7',
  'emerald-400': '#34d399',
  'emerald-500': '#10b981',
  'emerald-600': '#059669',
  'emerald-700': '#047857',
  'emerald-800': '#065f46',
  'emerald-900': '#064e3b',
  'emerald-950': '#022c22',
  'teal-50': '#f0fdfa',
  'teal-100': '#ccfbf1',
  'teal-200': '#99f6e4',
  'teal-300': '#5eead4',
  'teal-400': '#2dd4bf',
  'teal-500': '#14b8a6',
  'teal-600': '#0d9488',
  'teal-700': '#0f766e',
  'teal-800': '#115e59',
  'teal-900': '#134e4a',
  'teal-950': '#042f2e',
  'cyan-50': '#ecfeff',
  'cyan-100': '#cffafe',
  'cyan-200': '#a5f3fc',
  'cyan-300': '#67e8f9',
  'cyan-400': '#22d3ee',
  'cyan-500': '#06b6d4',
  'cyan-600': '#0891b2',
  'cyan-700': '#0e7490',
  'cyan-800': '#155e75',
  'cyan-900': '#164e63',
  'cyan-950': '#083344',
  'sky-50': '#f0f9ff',
  'sky-100': '#e0f2fe',
  'sky-200': '#bae6fd',
  'sky-300': '#7dd3fc',
  'sky-400': '#38bdf8',
  'sky-500': '#0ea5e9',
  'sky-600': '#0284c7',
  'sky-700': '#0369a1',
  'sky-800': '#075985',
  'sky-900': '#0c4a6e',
  'sky-950': '#082f49',
  'blue-50': '#eff6ff',
  'blue-100': '#dbeafe',
  'blue-200': '#bfdbfe',
  'blue-300': '#93c5fd',
  'blue-400': '#60a5fa',
  'blue-500': '#3b82f6',
  'blue-600': '#2563eb',
  'blue-700': '#1d4ed8',
  'blue-800': '#1e40af',
  'blue-900': '#1e3a8a',
  'blue-950': '#172554',
  'indigo-50': '#eef2ff',
  'indigo-100': '#e0e7ff',
  'indigo-200': '#c7d2fe',
  'indigo-300': '#a5b4fc',
  'indigo-400': '#818cf8',
  'indigo-500': '#6366f1',
  'indigo-600': '#4f46e5',
  'indigo-700': '#4338ca',
  'indigo-800': '#3730a3',
  'indigo-900': '#312e81',
  'indigo-950': '#1e1b4b',
  'violet-50': '#f5f3ff',
  'violet-100': '#ede9fe',
  'violet-200': '#ddd6fe',
  'violet-300': '#c4b5fd',
  'violet-400': '#a78bfa',
  'violet-500': '#8b5cf6',
  'violet-600': '#7c3aed',
  'violet-700': '#6d28d9',
  'violet-800': '#5b21b6',
  'violet-900': '#4c1d95',
  'violet-950': '#2e1065',
  'purple-50': '#faf5ff',
  'purple-100': '#f3e8ff',
  'purple-200': '#e9d5ff',
  'purple-300': '#d8b4fe',
  'purple-400': '#c084fc',
  'purple-500': '#a855f7',
  'purple-600': '#9333ea',
  'purple-700': '#7e22ce',
  'purple-800': '#6b21a8',
  'purple-900': '#581c87',
  'purple-950': '#3b0764',
  'fuchsia-50': '#fdf4ff',
  'fuchsia-100': '#fae8ff',
  'fuchsia-200': '#f5d0fe',
  'fuchsia-300': '#f0abfc',
  'fuchsia-400': '#e879f9',
  'fuchsia-500': '#d946ef',
  'fuchsia-600': '#c026d3',
  'fuchsia-700': '#a21caf',
  'fuchsia-800': '#86198f',
  'fuchsia-900': '#701a75',
  'fuchsia-950': '#4a044e',
  'pink-50': '#fdf2f8',
  'pink-100': '#fce7f3',
  'pink-200': '#fbcfe8',
  'pink-300': '#f9a8d4',
  'pink-400': '#f472b6',
  'pink-500': '#ec4899',
  'pink-600': '#db2777',
  'pink-700': '#be185d',
  'pink-800': '#9d174d',
  'pink-900': '#831843',
  'pink-950': '#500724',
  'rose-50': '#fff1f2',
  'rose-100': '#ffe4e6',
  'rose-200': '#fecdd3',
  'rose-300': '#fda4af',
  'rose-400': '#fb7185',
  'rose-500': '#f43f5e',
  'rose-600': '#e11d48',
  'rose-700': '#be123c',
  'rose-800': '#9f1239',
  'rose-900': '#881337',
  'rose-950': '#4c0519',
}

function colorWithOpacity(colorValue: string, opacity: number): string {
  if (colorValue === 'transparent') return 'transparent'
  if (colorValue === 'currentColor') return 'currentColor'

  if (colorValue.startsWith('#')) {
    const hex = colorValue.slice(1)
    if (hex.length === 3) {
      const r = Number.parseInt(hex[0] + hex[0], 16)
      const g = Number.parseInt(hex[1] + hex[1], 16)
      const b = Number.parseInt(hex[2] + hex[2], 16)
      return `rgba(${r},${g},${b},${opacity})`
    } else if (hex.length === 6) {
      const r = Number.parseInt(hex.slice(0, 2), 16)
      const g = Number.parseInt(hex.slice(2, 4), 16)
      const b = Number.parseInt(hex.slice(4, 6), 16)
      return `rgba(${r},${g},${b},${opacity})`
    }
  }

  return colorValue
}

function parseColorWithOpacity(value: string): string | null {
  const match = value.match(/^(.+?)\/(\d+|\[[\d.]+\])$/)
  if (!match) {
    const color = COLORS[value]
    return color || null
  }

  const [, colorName, opacityStr] = match
  const color = COLORS[colorName]
  if (!color) return null

  let opacity: number
  if (opacityStr.startsWith('[') && opacityStr.endsWith(']')) {
    opacity = Number.parseFloat(opacityStr.slice(1, -1))
  } else {
    opacity = Number.parseInt(opacityStr) / 100
  }

  return colorWithOpacity(color, opacity)
}

function parseArbitraryValue(value: string): string | null {
  const match = value.match(/^\[(.+)\]$/)
  return match ? match[1] : null
}

const SPACING: Record<string, string> = {
  0: '0',
  0.5: '0.125rem',
  1: '0.25rem',
  1.5: '0.375rem',
  2: '0.5rem',
  2.5: '0.625rem',
  3: '0.75rem',
  3.5: '0.875rem',
  4: '1rem',
  5: '1.25rem',
  6: '1.5rem',
  8: '2rem',
  10: '2.5rem',
  12: '3rem',
  16: '4rem',
  20: '5rem',
  24: '6rem',
}

function parseDynamicClass(className: string): string | null {
  if (TAILWIND_MAP[className]) {
    return TAILWIND_MAP[className]
  }

  const gradientDirs: Record<string, string> = {
    'bg-gradient-to-t':
      'background-image:linear-gradient(to top,var(--tw-gradient-stops))',
    'bg-gradient-to-tr':
      'background-image:linear-gradient(to top right,var(--tw-gradient-stops))',
    'bg-gradient-to-r':
      'background-image:linear-gradient(to right,var(--tw-gradient-stops))',
    'bg-gradient-to-br':
      'background-image:linear-gradient(to bottom right,var(--tw-gradient-stops))',
    'bg-gradient-to-b':
      'background-image:linear-gradient(to bottom,var(--tw-gradient-stops))',
    'bg-gradient-to-bl':
      'background-image:linear-gradient(to bottom left,var(--tw-gradient-stops))',
    'bg-gradient-to-l':
      'background-image:linear-gradient(to left,var(--tw-gradient-stops))',
    'bg-gradient-to-tl':
      'background-image:linear-gradient(to top left,var(--tw-gradient-stops))',
    'bg-linear-to-t':
      'background-image:linear-gradient(to top,var(--tw-gradient-stops))',
    'bg-linear-to-tr':
      'background-image:linear-gradient(to top right,var(--tw-gradient-stops))',
    'bg-linear-to-r':
      'background-image:linear-gradient(to right,var(--tw-gradient-stops))',
    'bg-linear-to-br':
      'background-image:linear-gradient(to bottom right,var(--tw-gradient-stops))',
    'bg-linear-to-b':
      'background-image:linear-gradient(to bottom,var(--tw-gradient-stops))',
    'bg-linear-to-bl':
      'background-image:linear-gradient(to bottom left,var(--tw-gradient-stops))',
    'bg-linear-to-l':
      'background-image:linear-gradient(to left,var(--tw-gradient-stops))',
    'bg-linear-to-tl':
      'background-image:linear-gradient(to top left,var(--tw-gradient-stops))',
  }
  if (gradientDirs[className]) {
    return gradientDirs[className]
  }

  if (className.startsWith('from-')) {
    const value = className.slice(5)
    const color = parseColorWithOpacity(value)
    if (color) {
      return `--tw-gradient-from:${color};--tw-gradient-to:transparent;--tw-gradient-stops:var(--tw-gradient-from),var(--tw-gradient-to)`
    }
  }

  if (className.startsWith('via-')) {
    const value = className.slice(4)
    const color = parseColorWithOpacity(value)
    if (color) {
      return `--tw-gradient-to:transparent;--tw-gradient-stops:var(--tw-gradient-from),${color},var(--tw-gradient-to)`
    }
  }

  if (className.startsWith('to-')) {
    const value = className.slice(3)
    const color = parseColorWithOpacity(value)
    if (color) {
      return `--tw-gradient-to:${color}`
    }
  }

  if (className.startsWith('bg-') && !className.startsWith('bg-gradient')) {
    const value = className.slice(3)
    const arbitrary = parseArbitraryValue(value)
    if (arbitrary) {
      return `background-color:${arbitrary}`
    }
    const color = parseColorWithOpacity(value)
    if (color) {
      return `background-color:${color}`
    }
  }

  if (className.startsWith('text-')) {
    const value = className.slice(5)
    const arbitrary = parseArbitraryValue(value)
    if (arbitrary) {
      if (
        arbitrary.includes('var(') ||
        arbitrary.startsWith('#') ||
        arbitrary.includes('rgb')
      ) {
        return `color:${arbitrary}`
      }
      return `font-size:${arbitrary}`
    }
    const color = parseColorWithOpacity(value)
    if (color) {
      return `color:${color}`
    }
  }

  if (
    className.startsWith('border-') &&
    !/^border-([trblxy])?-?\d/.test(className)
  ) {
    const value = className.slice(7)
    const arbitrary = parseArbitraryValue(value)
    if (arbitrary) {
      return `border-color:${arbitrary}`
    }
    const color = parseColorWithOpacity(value)
    if (color) {
      return `border-color:${color}`
    }
  }

  if (className.startsWith('placeholder-')) {
    const value = className.slice(12)
    const color = parseColorWithOpacity(value)
    if (color) {
      return `--tw-placeholder-color:${color}`
    }
  }

  if (className.startsWith('max-w-')) {
    const value = className.slice(6)
    const arbitrary = parseArbitraryValue(value)
    if (arbitrary) {
      return `max-width:${arbitrary}`
    }
  }

  if (className.startsWith('min-h-')) {
    const value = className.slice(6)
    const arbitrary = parseArbitraryValue(value)
    if (arbitrary) {
      return `min-height:${arbitrary}`
    }
  }

  if (className.startsWith('w-')) {
    const value = className.slice(2)
    const arbitrary = parseArbitraryValue(value)
    if (arbitrary) {
      return `width:${arbitrary}`
    }
  }

  if (className.startsWith('h-')) {
    const value = className.slice(2)
    const arbitrary = parseArbitraryValue(value)
    if (arbitrary) {
      return `height:${arbitrary}`
    }
  }

  if (className.startsWith('gap-')) {
    const value = className.slice(4)
    const arbitrary = parseArbitraryValue(value)
    if (arbitrary) {
      return `gap:${arbitrary}`
    }
  }

  const paddingMatch = className.match(/^p([xytrbl])?-\[(.+)\]$/)
  if (paddingMatch) {
    const [, side, val] = paddingMatch
    switch (side) {
      case 'x':
        return `padding-left:${val};padding-right:${val}`
      case 'y':
        return `padding-top:${val};padding-bottom:${val}`
      case 't':
        return `padding-top:${val}`
      case 'r':
        return `padding-right:${val}`
      case 'b':
        return `padding-bottom:${val}`
      case 'l':
        return `padding-left:${val}`
      default:
        return `padding:${val}`
    }
  }

  const marginMatch = className.match(/^m([xytrbl])?-\[(.+)\]$/)
  if (marginMatch) {
    const [, side, val] = marginMatch
    switch (side) {
      case 'x':
        return `margin-left:${val};margin-right:${val}`
      case 'y':
        return `margin-top:${val};margin-bottom:${val}`
      case 't':
        return `margin-top:${val}`
      case 'r':
        return `margin-right:${val}`
      case 'b':
        return `margin-bottom:${val}`
      case 'l':
        return `margin-left:${val}`
      default:
        return `margin:${val}`
    }
  }

  if (className.startsWith('rounded-')) {
    const value = className.slice(8)
    const arbitrary = parseArbitraryValue(value)
    if (arbitrary) {
      return `border-radius:${arbitrary}`
    }
  }

  if (className.startsWith('max-h-')) {
    const value = className.slice(6)
    const arbitrary = parseArbitraryValue(value)
    if (arbitrary) {
      return `max-height:${arbitrary}`
    }
  }

  return null
}

/** 类名里除字母数字、`_`、`-` 外都转义，反斜杠也算；漏转的 `#` 会让 `bg-[#fff]` 对不上元素，漏转的反斜杠会吞掉后面的 `{`。 */
function escapeClassSelector(className: string): string {
  return className.replaceAll(/[^\w-]/g, '\\$&')
}

export function generateOnDemandTailwindCSS(html: string): string {
  const usedClasses = extractClassNames(html)
  const cssRules: string[] = []
  const placeholderRules: string[] = []
  const spaceRules: string[] = []

  cssRules.push(
    '*,::before,::after{--tw-gradient-from:#fff;--tw-gradient-to:transparent;--tw-gradient-stops:var(--tw-gradient-from),var(--tw-gradient-to)}',
  )

  const needsAnimations = Iterator.from(usedClasses).some((c) =>
    c.includes('animate-'),
  )
  if (needsAnimations) {
    cssRules.push('@keyframes spin{to{transform:rotate(360deg)}}')
    cssRules.push('@keyframes ping{75%,100%{transform:scale(2);opacity:0}}')
    cssRules.push('@keyframes pulse{50%{opacity:.5}}')
    cssRules.push(
      '@keyframes bounce{0%,100%{transform:translateY(-25%);animation-timing-function:cubic-bezier(0.8,0,1,1)}50%{transform:none;animation-timing-function:cubic-bezier(0,0,0.2,1)}}',
    )
  }

  usedClasses.forEach((className) => {
    let baseClass = className
    let prefix = ''
    let selector = ''
    let isGroupHover = false

    if (baseClass.startsWith('dark:')) {
      baseClass = baseClass.slice(5)
      prefix = '.dark '
    }

    if (baseClass.startsWith('group-hover:')) {
      baseClass = baseClass.slice(12)
      isGroupHover = true
    }

    if (baseClass.startsWith('hover:')) {
      baseClass = baseClass.slice(6)
      selector = ':hover'
    }

    if (baseClass.startsWith('focus:')) {
      baseClass = baseClass.slice(6)
      selector = ':focus'
    }

    if (baseClass.startsWith('active:')) {
      baseClass = baseClass.slice(7)
      selector = ':active'
    }

    if (baseClass.startsWith('disabled:')) {
      baseClass = baseClass.slice(9)
      selector = ':disabled'
    }

    if (baseClass.startsWith('focus-within:')) {
      baseClass = baseClass.slice(13)
      selector = ':focus-within'
    }

    const spaceYMatch = baseClass.match(/^space-y-(\d+(?:\.\d+)?)$/)
    if (spaceYMatch) {
      const spacingValue = SPACING[spaceYMatch[1]]
      if (spacingValue) {
        const escapedClass = escapeClassSelector(className)
        spaceRules.push(
          `${prefix}.${escapedClass}>:not([hidden])~:not([hidden]){margin-top:${spacingValue}}`,
        )
      }
      return
    }

    const spaceXMatch = baseClass.match(/^space-x-(\d+(?:\.\d+)?)$/)
    if (spaceXMatch) {
      const spacingValue = SPACING[spaceXMatch[1]]
      if (spacingValue) {
        const escapedClass = escapeClassSelector(className)
        spaceRules.push(
          `${prefix}.${escapedClass}>:not([hidden])~:not([hidden]){margin-left:${spacingValue}}`,
        )
      }
      return
    }

    const css = parseDynamicClass(baseClass)
    if (css) {
      const escapedClass = escapeClassSelector(className)

      if (isGroupHover) {
        cssRules.push(`${prefix}.group:hover .${escapedClass}{${css}}`)
      }
      else if (baseClass.startsWith('placeholder-')) {
        placeholderRules.push(
          `${prefix}.${escapedClass}::placeholder{color:var(--tw-placeholder-color)}`,
        )
        cssRules.push(`${prefix}.${escapedClass}${selector}{${css}}`)
      } else {
        cssRules.push(`${prefix}.${escapedClass}${selector}{${css}}`)
      }
    }
  })

  return cssRules.concat(spaceRules).concat(placeholderRules).join('\n')
}

const themeCSSCache = new Map<string, string>()

export function generateThemeCSS(
  isDark: boolean,
  primaryColor: string,
): string {
  const cacheKey = `${isDark ? 'd' : 'l'}-${primaryColor}`

  const cached = themeCSSCache.get(cacheKey)
  if (cached) return cached

  if (themeCSSCache.size >= 20) {
    const firstKey = themeCSSCache.keys().next().value
    if (firstKey) themeCSSCache.delete(firstKey)
  }

  const hexToRgb = (hex: string): string => {
    const h = hex.replaceAll('#', '')
    const r = Number.parseInt(h.length === 3 ? h[0] + h[0] : h.slice(0, 2), 16)
    const g = Number.parseInt(h.length === 3 ? h[1] + h[1] : h.slice(2, 4), 16)
    const b = Number.parseInt(h.length === 3 ? h[2] + h[2] : h.slice(4, 6), 16)
    return `${r}, ${g}, ${b}`
  }

  const primaryRgb = hexToRgb(primaryColor)

  const css = `
:root {
  /* 壁纸主色（从系统传入） */
  --tapp-primary: ${primaryColor};
  --tapp-primary-rgb: ${primaryRgb};

  /* 语义色彩变量（供 Tapp 使用） */
  --text-primary: ${isDark ? 'rgba(255,255,255,.92)' : '#1a1a1a'};
  --text-secondary: ${isDark ? 'rgba(255,255,255,.5)' : '#999'};
  --bg-primary: ${isDark ? '#0a0a0a' : '#fff'};

  /* 缩放因子（由容器动态更新） */
  --tapp-scale: 1;
  --tapp-font-scale: 1;
}

/* 暗色模式 */
.dark {
  color-scheme: dark;
  --text-primary: rgba(255,255,255,.92);
  --text-secondary: rgba(255,255,255,.5);
  --bg-primary: #0a0a0a;
}
body.dark { background: var(--bg-primary); color: var(--text-primary); }
body.light { background: var(--bg-primary); color: var(--text-primary); }
`

  themeCSSCache.set(cacheKey, css)
  return css
}

export const BASE_CSS = `
*, *::before, *::after {
  box-sizing: border-box;
  margin: 0;
  padding: 0;
}

html, body {
  width: 100%;
  height: 100%;
  overflow: hidden;
  font-family: system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  font-size: 14px;
  line-height: 1.5;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  background: var(--bg-primary, #fff);
  color: var(--text-primary, #1a1a1a);
}
`

export const WIDGET_CSS = `
body {
  background: transparent;
}

#widget-root {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  overflow: hidden;
}
`

export const PAGE_CSS = `
#tapp-root {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  overflow: hidden;
}

#tapp-background {
  position: absolute;
  inset: 0;
  z-index: 0;
}

#tapp-content {
  position: relative;
  z-index: 1;
  width: 100%;
  height: 100%;
  box-sizing: border-box;
  overflow: auto;
}
`

export const WIDGET_STATIC_CSS = `${BASE_CSS}${WIDGET_CSS}` as const

export const PAGE_STATIC_CSS = `${BASE_CSS}${PAGE_CSS}` as const
