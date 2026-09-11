#ifndef CAPY_APPLE_H
#define CAPY_APPLE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

typedef struct CapyApple CapyApple;
/* All session calls run on one serial engine/render owner. UIKit/AppKit owns
   the retained CAMetalLayer and must keep it alive until detach completes. */
CapyApple *capy_apple_create(uint32_t platform); /* 0 iPadOS, 1 macOS */
void capy_apple_destroy(CapyApple *app);
const char *capy_apple_error(const CapyApple *app); /* borrowed until next call */
void capy_apple_string_free(char *text);
/* request: 0 action, 1 UI input, 2 query, 3 changed snapshot, 4 numeric control.
   Returned JSON is owned; release using capy_apple_string_free. NULL is either
   no changed snapshot or failure (consult capy_apple_error). */
char *capy_apple_request(CapyApple *app, uint32_t request, const char *json);
int32_t capy_apple_attach(CapyApple *app, void *metal_layer,
                         uint32_t width, uint32_t height, float scale);
int32_t capy_apple_resize(CapyApple *app, uint32_t width, uint32_t height, float scale);
int32_t capy_apple_detach(CapyApple *app);
/* Nine doubles per record: x/y physical pixels, pressure, tilt x/y radians,
   twist radians, distance, monotonic nanoseconds, phase (0 hover..4 cancel).
   tool: 0 pen, 1 mouse, 2 eraser, 3 touch; button: 0 primary, 1 pan, 2 other. */
int32_t capy_apple_pointer(CapyApple *app, uint64_t id, uint32_t tool, uint32_t button,
                          const double *records, size_t count, uint32_t predicted,
                          uint64_t view_revision);
/* Returns 1 if more frames are needed, 0 when idle, -1 on error. Optional costs
   receives 5 nanosecond durations: paint, acquire, viewport, present, poll. */
int32_t capy_apple_frame(CapyApple *app, uint64_t now_ns, uint64_t presentation_ns,
                        uint64_t *costs);
uint64_t capy_apple_camera_revision(const CapyApple *app);
#ifdef __cplusplus
}
#endif
#endif
