# High tier (optional): 120 fps on 36 MP

[Performance targets](../PERFORMANCE_TARGETS.md)

Reference: Samsung Galaxy Tab S7 (SM-T878U) with a 1600 × 2560, 120 Hz panel.
The canvas is 7360 × 4912.

- Every row targets **120 fps** unless marked soft.
- The tier is optional. A change need not measure it or meet it to qualify;
  hardware between Mid and Top must still meet the
  [Mid targets](mid-tier.md).
- Android 13's Vulkan loader has no swapchain maintenance, so ink uses
  [FIFO presentation](../development/gpu-fallbacks.md#android-presentation).
- No measurements are recorded yet.
