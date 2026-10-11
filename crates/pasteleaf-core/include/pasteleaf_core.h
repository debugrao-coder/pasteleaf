#ifndef PASTELEAF_CORE_H
#define PASTELEAF_CORE_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* API 2. UTF-8 JSON envelopes {ok,data} or {ok:false,error:{code,message}}.
 * Handle owns a serialized worker. Never close a handle while a call is active.
 * pl_open returns NULL on failure; pl_request(NULL, ...) returns an error.
 * Returned strings belong to the core and MUST be released with pl_free.
 * Do not free a handle or returned string using the host allocator.
 * Calls may block for disk/crypto I/O; invoke from a background UI queue. */
uint32_t pl_api_version(void);
void *pl_open(const char *root);
char *pl_request(void *handle, const char *json);
void pl_free(char *json);
void pl_close(void *handle);
#ifdef __cplusplus
}
#endif
#endif
