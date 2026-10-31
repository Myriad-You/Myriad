import type { RuntimeModuleSource } from './sandbox/hostRuntimeModules'
import type { TappResourceSource } from './sandbox/resourceLoader'
import type {
  TappBridgeQuota,
  TappFederationLimitsSource,
  TappMessageTarget,
} from './TappBridge'
import type { TappGrantTransport } from './TappRuntimeGrant'
import type { TappInstance } from './types'
import { HostRuntimeModules } from './sandbox/hostRuntimeModules'
import { TappResourceLoader } from './sandbox/resourceLoader'
import {
  cspOptionsFromPermissions,
  escapeSandboxScriptSource,
  generateCSP,
  generateSecurityWrapper,
  sandboxScriptNonce,
} from './sandbox/security'
import { TappBridge, TappBridgeRouter } from './TappBridge'
import { TappGrantRegistry } from './TappRuntimeGrant'

/**
 * What the host lends the kernel. The kernel reads nothing from the page
 * itself: no location, no document, no locale, no host API module.
 */
export interface TappKernelHost {
  grants: TappGrantTransport
  fetchResources: TappResourceSource
  fetchRuntimeModule: RuntimeModuleSource
  quota: TappBridgeQuota
  federation: TappFederationLimitsSource
  /** Text for an error a bridge handler threw; the host words and redacts it. */
  describeError: (error: unknown) => string
  /** The origin sandbox images and media may load from (the site's own origin). */
  assetOrigin: () => string
  /** The host document's CSP nonce, if it has one. */
  cspNonce: () => string | undefined
  /** Where sandbox messages arrive; null where there is no window (tests). */
  messageTarget: TappMessageTarget | null
}

/** What a sandbox document's scripts and CSP are built from; one set per document. */
export interface TappDocumentSecurity {
  nonce: string
  csp: string
  /** The security wrapper script, already escaped for an inline <script>. */
  securityWrapper: string
}

/**
 * The Tapp runtime every host shares. One per page, built by the host: it owns
 * the page's grants, bridge routing, package resource cache and runtime
 * modules, so nothing in the kernel keeps module-level state.
 */
export class TappKernel {
  readonly grants: TappGrantRegistry
  readonly resources: TappResourceLoader
  readonly runtimeModules: HostRuntimeModules
  private readonly router: TappBridgeRouter

  constructor(private readonly host: TappKernelHost) {
    this.grants = new TappGrantRegistry(host.grants)
    this.resources = new TappResourceLoader(host.fetchResources)
    this.runtimeModules = new HostRuntimeModules(host.fetchRuntimeModule)
    this.router = new TappBridgeRouter(host.messageTarget)
  }

  createBridge(): TappBridge {
    return new TappBridge({
      router: this.router,
      grants: this.grants,
      quota: this.host.quota,
      federation: this.host.federation,
      describeError: this.host.describeError,
    })
  }

  assetOrigin(): string {
    return this.host.assetOrigin()
  }

  /** The nonce sandbox scripts carry: the host's, or a random one where it has none. */
  scriptNonce(): string {
    return sandboxScriptNonce(this.host.cspNonce())
  }

  /** Nonce, CSP and security wrapper for one sandbox document. The wrapper's image checks and the CSP read the same options. */
  documentSecurity(
    instance: Pick<TappInstance, 'grantedPermissions' | 'grantedRemoteMedia'>,
    sessionToken: string,
  ): TappDocumentSecurity {
    const nonce = this.scriptNonce()
    const hostOrigin = this.assetOrigin()
    const options = {
      ...cspOptionsFromPermissions(instance.grantedPermissions, instance.grantedRemoteMedia),
      hostOrigin,
    }
    return {
      nonce,
      csp: generateCSP(nonce, options),
      securityWrapper: escapeSandboxScriptSource(
        generateSecurityWrapper(sessionToken, options.remoteMediaHosts, hostOrigin),
      ),
    }
  }
}
