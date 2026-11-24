# 0004: Renderer on Vulkan 1.3 via ash

- Status: accepted
- Date: 2026-11-24
- Owner answer reference: 3.2, 9.2

## Decision
Use `ash` with Vulkan 1.3 core features (dynamic rendering, synchronization2, descriptor indexing). Vanilla GLSL core
shaders, read from the jar or from resource packs, are compiled with shaderc to SPIR-V at runtime. macOS runs on
MoltenVK. An OpenGL backend may be added later.

## Consequences
+ Maximum control (needed for pixel-exact output) and speed (GPU-driven rendering).
− More low-level code than wgpu; GPUs without Vulkan 1.3 are unsupported at first.
