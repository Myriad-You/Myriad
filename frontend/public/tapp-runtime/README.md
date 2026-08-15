# Tapp host runtime

`three.0.170.iife.js` is a pinned Three.js r170 + GLTFLoader IIFE.

The host fetches it and nonce-inlines it into a Page sandbox only when the
manifest declares `runtimeModules: ["three"]`. It is not part of the Myriad
application bundle and is never loaded from a CDN.

Rebuild (from a tree that can resolve `three@0.170`):

```bash
node frontend/scripts/bundle-tapp-three.mjs
```
