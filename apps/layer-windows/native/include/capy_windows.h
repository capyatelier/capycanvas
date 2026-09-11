#pragma once
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif
typedef struct CapyHost CapyHost;
typedef struct CapyPointer {
    uint64_t id, timestamp_ns, sequence, view_revision;
    float x, y, pressure, tilt_x, tilt_y, twist, distance;
    uint32_t phase; /* hover=0, down=1, move=2, up=3, cancel=4 */
    uint32_t tool;  /* pen=0, mouse=1, eraser=2, touch=3 */
    uint32_t button; /* primary=0, pan=1, other=2 */
    uint32_t flags; /* shared predicted/primary/barrel/inverted bits */
} CapyPointer;

/* Create on XAML thread with Microsoft ISwapChainPanelNative interface pointer.
   Then transfer exclusive ownership to the canvas thread. */
__declspec(dllimport) CapyHost* capy_create(void* panel, uint32_t width, uint32_t height, float scale);
__declspec(dllimport) void capy_destroy(CapyHost*);
__declspec(dllimport) const char* capy_error(void);
__declspec(dllimport) int32_t capy_resize(CapyHost*, uint32_t width, uint32_t height, float scale);
__declspec(dllimport) int32_t capy_pointer(CapyHost*, const CapyPointer*, size_t count);
__declspec(dllimport) int32_t capy_action(CapyHost*, const char* json);
__declspec(dllimport) int32_t capy_input(CapyHost*, const char* json);
__declspec(dllimport) int32_t capy_suspend(CapyHost*);
__declspec(dllimport) uint64_t capy_view_revision(const CapyHost*);
/* Acquisition: 2=UI-thread reconfiguration required. Can wait for DXGI. Drain newly arrived input after this call. */
__declspec(dllimport) int32_t capy_acquire(CapyHost*);
__declspec(dllimport) int32_t capy_frame(CapyHost*, uint64_t now_ns, uint64_t presentation_ns);
/* Returned UTF-8 owned by Rust; release with capy_string_free. Null=no change. */
__declspec(dllimport) char* capy_snapshot(CapyHost*);
__declspec(dllimport) void capy_string_free(char*);
#ifdef __cplusplus
}
#endif
