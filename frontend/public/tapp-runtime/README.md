# Tapp host runtime

`three.0.170.iife.js` is the source of truth for host-injected Three.js r170 +
GLTFLoader. SHA-256:

```
0ca6ee7e41840a8b95d416f7f38b204126838f277f248a1761acdb7662f2b60d
```

The host fetches it and nonce-inlines it into a Page sandbox only when the
manifest declares `runtimeModules: ["three"]`. It is not part of the Myriad
application bundle and is never loaded from a CDN.

`frontend/scripts/bundle-tapp-three.mjs` can rebuild the blob if you have
`three@0.170` and `esbuild` available. Those packages are intentionally not
frontend production dependencies.
