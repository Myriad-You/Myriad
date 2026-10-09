/*
 * Myriad shared core: one call in, one owned buffer out.
 *
 * `myriad_core_call` runs the pure function `name` (NUL-terminated UTF-8)
 * on `len` bytes of JSON at `input` (null with `len` 0 means `{}`) and
 * returns JSON in a buffer the caller owns. Free it once with
 * `myriad_core_buf_free`. `status` is 0 on success; otherwise the buffer
 * holds `{"error":{"kind":...,"message":...}}`. `meta.version` names the
 * ABI version, the upstream commit and every call.
 */
#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct MyriadCoreBuf {
    uint8_t *ptr;
    size_t len;
    /* 0 ok, 1 unknown call, 2 bad input, 3 panic inside the core. */
    int32_t status;
} MyriadCoreBuf;

MyriadCoreBuf myriad_core_call(const char *name, const uint8_t *input, size_t len);
void myriad_core_buf_free(MyriadCoreBuf buf);

#ifdef __cplusplus
}
#endif
