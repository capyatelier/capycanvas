# Tier hardware

[Performance targets](../PERFORMANCE_TARGETS.md)

The reference devices below define the tiers. The per-device RAM, display and
CPU figures come from the attached units, read over `adb`. Benchmark scores are
SoC-level medians from notebookcheck and nanoreview; Geekbench Browser was not
reachable.

| | Low | Mid | Top |
| --- | --- | --- | --- |
| Device | TCL TAB 11 Gen 2 or NXTPAPER 11 Gen 2 (model 9465X) | Wacom MovinkPad 11 (DTHA116) | Wacom MovinkPad Pro 14 (DTHA140) |
| Android | 15 | 14 | 15 |
| SoC | MediaTek MT8786 (Helio G80 class), 12 nm | MediaTek MT8781 (Helio G99 class), 6 nm | Qualcomm Snapdragon 8s Gen 3 (SM8635), 4 nm |
| CPU | 2 × Cortex-A75 2.0 GHz + 6 × A55 1.8 GHz | 2 × Cortex-A76 2.2 GHz + 6 × A55 2.0 GHz | 1 × Cortex-X4 3.0 GHz + 4 × A720 2.8 GHz + 3 × A520 2.0 GHz |
| GPU | Arm Mali-G52 MC2 (Bifrost), 950 MHz | Arm Mali-G57 MC2 (Valhall), about 1.0 GHz | Qualcomm Adreno 735, 1.1 GHz |
| FP32 lanes | 48 (2 cores × 3 engines × 8) | 64 (2 cores × 2 engines × 16) | 768 (3 SPs × 256) |
| Peak FP32 | 91 GFLOPS | 128 GFLOPS | 1.69 TFLOPS |
| Register file | About 96 KiB: 16 KiB per execution engine (derived) | Not published | About 576 KiB: 192 KiB per SP (derived from Mesa) |
| On-chip tile memory | About 8 KiB per core | About 8 KiB per core (derived) | 1.5 MiB GMEM |
| Memory | LPDDR4X-3600, 2 × 16-bit, 14.4 GB/s | LPDDR4X-4266, 2 × 16-bit, 17.1 GB/s | LPDDR5X-8400, 4 × 16-bit, 67.2 GB/s |
| RAM | 6 GB | 8 GB | 12 GB |
| Display | 10.95 in IPS, 1200 × 1920, 60 Hz | 11.45 in IPS, 1440 × 2200, 60/90 Hz | 14 in OLED, 1800 × 2880, 60/120 Hz |
| Pen | TCL T-Pen, active capacitive, 4096 levels | Wacom Pro Pen 3, EMR, 8192 levels | Wacom Pro Pen 3, EMR, 8192 levels |
| Geekbench 6 CPU (single / multi) | 406 / 1456 | 727 / 2047 | 1923 / 5179 (this unit: 2016 / 5335) |
| Geekbench 6 GPU (Vulkan / OpenCL) | 997 / 1065 | 1294 / 1410 | 9942 / 9159 |
| 3DMark Wild Life Extreme | 172 | 335 | 3076 |
| 3DMark Steel Nomad Light | not published | 132 | 1064 |
| GFXBench Aztec Ruins Normal, offscreen | 9.2 fps | 16 fps | 122 fps |
| Comparable hardware | Snapdragon 680 phones; GPU about a third of an Apple A10 | Snapdragon 720G class (Adreno 618); GPU about half an Apple A10 | Intel Iris Xe (96 EU) laptop; GPU about 65% of an M1 iPad Pro |

**How to place your hardware in a tier:**

- **Compare memory bandwidth and GPU throughput first.** Large brushes,
  composition and filters are bandwidth-bound.
- **Tile memory affects bandwidth on Mali.** Both Mali GPUs keep little tile
  memory, so every full-screen pass goes through DRAM.
- **The mid tier is closer to the low tier than to the top.** Its GPU throughput
  is only 1.4–2 times the low tier's, and its bandwidth 1.2 times. Yet it must
  draw 1.5 times the frame rate on twice the canvas.
- **The top tier leaves more room.** It has about 18 times the low tier's GPU
  throughput and 4.7 times its bandwidth.
- **Desktop and Apple hosts use the same tables.** Choose the tier that the
  device's GPU and bandwidth meet or exceed:
  - A 2021 or newer iPad Pro or M-series Mac exceeds the top tier.
  - An Intel Iris Xe laptop roughly matches it.

**Sources:**

- MediaTek [MT8786](https://www.mediatek.com/iot/modem-based-iot/mt8786) and [MT8781](https://www.mediatek.com/iot/modem-based-iot/mt8781).
- The [Snapdragon 8s Gen 3 product brief](https://docs.qualcomm.com/bundle/publicresource/87-73942-1_REV_C_Snapdragon_8s_Gen_3_Mobile_Platform_Product_Brief.pdf).
- Wacom [MovinkPad 11](https://www.wacom.com/en-us/products/wacom-movinkpad-11) and [MovinkPad Pro 14](https://www.wacom.com/en-us/products/wacom-movinkpad-pro-14).
- [TCL TAB 11 Gen 2](https://www.tcl.com/global/en/tablets/tcl-tab-11-gen-2).
- Notebookcheck [Mali-G52 MP2](https://www.notebookcheck.net/ARM-Mali-G52-MP2-GPU-Benchmarks-and-Specs.466940.0.html), [Mali-G57 MP2](https://www.notebookcheck.net/ARM-Mali-G57-MP2-GPU-Benchmarks-and-Specs.537758.0.html) and [Adreno 735](https://www.notebookcheck.net/Qualcomm-Adreno-735-Benchmarks-and-Specs.857242.0.html).
- nanoreview [Helio G80](https://nanoreview.net/en/soc/mediatek-helio-g80), [Helio G99](https://nanoreview.net/en/soc/mediatek-helio-g99) and [Snapdragon 8s Gen 3](https://nanoreview.net/en/soc/qualcomm-snapdragon-8s-gen-3).
- [Chips and Cheese on Bifrost](https://chipsandcheese.com/p/arms-bifrost-architecture-and-the).
- The [Arm Valhall optimization guide](https://documentation-service.arm.com/static/60b65adde022752339b44b7d).
- Mesa [`freedreno_devices.py`](https://gitlab.freedesktop.org/mesa/mesa/-/blob/main/src/freedreno/common/freedreno_devices.py) and [`ir3_compiler.h`](https://gitlab.freedesktop.org/mesa/mesa/-/blob/main/src/freedreno/ir3/ir3_compiler.h).
- The Pro 14 device scores come from [GIGAZINE](https://gigazine.net/gsc_news/en/20251115-wacom-movinkpad-pro-14-benchmark/).

GPU clocks come from the SoC specifications. The tablets' frequency tables need
root to read.
