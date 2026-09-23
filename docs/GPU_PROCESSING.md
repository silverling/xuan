# GPU processing audit

The editor installs a shared wgpu processing context on the UI thread and carries
it into filter, retouch, and RAW workers. Compute is preferred for substantial
pixel work when a hardware adapter is available. The library's CPU implementations
remain the reference and fallback; standalone callers without a GPU context
continue to work.

## Routing by module

| Module / operation | Execution |
| --- | --- |
| All four raster filters | GPU Gaussian Blur, Motion Blur, Add Noise, and Lens Correction; selection blending and Gaussian mask targets also use compute |
| Every adjustment variant | GPU Hue/Saturation, hue ranges/colorize, Levels/channel levels, Curves/channel curves, Exposure, Gradient Map, Film Grain, Grain, and Invert; destructive pixels, masks, and adjustment layers |
| Canvas composition | GPU blending, adjustments, masks, inherited coverage, and clipping; cached source textures and mipmaps |
| Export, merge, copy merged, isolated retouch rasters | Full requested resolution on GPU, with straight-alpha readback; independent of the canvas preview cap |
| Image resampling | GPU separable premultiplied Lanczos3; Triangle resampling for masks, selections, and floating-point RAW proxies |
| Mask processing | GPU Gaussian feathering, transformed selection projection, alpha/mask selections, clipping bake when copying layers, and background-removal matte smoothing/masking |
| RAW development | GPU geometry, lens correction, chromatic aberration, white balance, camera matrix, exposure, vignette, denoise, local overlays, tone mapping, curves, HSL, monochrome, split toning, clarity, texture, and sharpening |
| RAW previews | Resident camera inputs and reusable float/packed buffers; native display textures, tiled histogram reduction, and a 4,112-byte histogram readback; optional GPU clipping textures |
| RAW output | GPU crop and quantization to 8-bit or true 16-bit RGBA; floating-point intermediates stay on the GPU until final readback |
| Painting | GPU large fills, erasure, linear/radial gradients, curved shapes, and large paint/erase/clone/smudge/blur/heal strokes; stroke readback covers only the affected rectangle |
| Selection color matching | GPU noncontiguous Magic Wand; contiguous connectivity traversal remains CPU |
| Analysis | GPU combined RAW RGB histograms, clipping counts, and warning pixels |
| CPU analysis | Integer luminance histogram / Auto Levels (measured faster than GPU transfers), small Levels histograms, sparse Auto Exposure samples, 7×7 white-balance samples, and point hit tests |
| Selection spans and memory operations | CPU rectangle/ellipse span rasterization, polygon scan conversion, combine, translate, flip, bounds, crop/copy, alpha multiplication for clipboard extraction, and flat-background encoding |
| Content-aware fill and background connectivity | CPU ordered frontier/patch search and edge-seeded flood fill; GPU composition and matte processing surround those stages |
| Text, imports, project management | CPU font discovery, shaping/glyph rasterization, codecs/RAW decoding and demosaicing, metadata, transforms, history, file/clipboard I/O, and UI |

Small or local edits stay on CPU to avoid upload, dispatch, and readback overhead.
Neighborhood operations generally start using compute at 16,384 pixels; pointwise
edits generally start at 65,536 pixels. Brush thresholds use the affected rectangle,
not the entire document. Rectangles and polygon selections already fill contiguous
CPU spans. Simple memory copies and small reductions generally cost less than a GPU
round trip. The integer luminance histogram also stays on CPU: at 1600×1200 it
measured 2.6 ms against 6.0 ms for a GPU round trip. RAW previews reduce their three channel histograms directly from resident output buffers. Each workgroup processes a 32×32 tile; warning pixels are generated only when requested. The UI registers completed native textures without full-image readback, analysis, or color conversion. CPU fallback prepares display pixels and analysis in the worker.

The existing inpainting and flood-fill implementations consume results from earlier
frontier visits. Moving their inner loops to GPU would require repeated dispatches
and synchronization, or a different algorithm with different results. Their
independent image-wide preparation and postprocessing use compute.

## Execution and fidelity

Attached image effects are materialized in source coordinates before composition,
using the existing GPU filter and adjustment routines when available. Mask
coverage and effect opacity are combined in premultiplied color on the CPU. A
standalone filter materializes its accumulated backdrop with the CPU compositor,
runs the raster filter, and returns the result through the regular display path.
The source image and saved effect settings remain unchanged. This path prioritizes
the same bottom-to-top result in preview, export, merging, and clipboard copies;
long effect stacks can increase recomposition time.

- Full-resolution processing uses original image dimensions. Preview downsampling
  never sets the Apply/export resolution.
- Device limits are checked before buffer/texture work. The desktop requests the
  adapter's supported storage-buffer and texture limits instead of imposing wgpu's
  portable 128 MiB binding limit. Resource failures, unavailable compute, and GPU
  errors use CPU fallback. Software adapters are not selected for acceleration.
- Pipelines are cached. GPU context and cancellation are scoped to each worker;
  they do not change other library callers or CPU reference tests. Readback polling
  observes cancellation, and existing worker revisions/history govern committing
  results.
- RAW passes retain floating-point intermediates through final 8/16-bit encoding.
  Brush overlays bin dab centers into tiles so pixels visit only overlapping dabs.
- Each RAW preview worker caches at most two camera inputs and their reusable
  processing buffers. Outputs are immutable textures, so a cancelled or later
  render cannot change an image already displayed. Switching tabs releases compute
  buffers off the UI thread. Exports retain the full-resolution materialized path.
- Interactive RAW requests start at most every 33 ms, without waiting for a drag
  to end. Unless full resolution is forced, edits use the small proxy before
  refining to the display's resolution after 150 ms of inactivity. Preview levels
  grow by 1.5× from the proxy, capped at source and texture limits. Original images
  are rendered lazily for comparisons, and cached independently of adjustments.
- GPU and CPU use the same adjustment math. Premultiplication, Gaussian kernels,
  edge handling, Lanczos weights, selection coverage, bounds expansion, mask
  placement, and alpha preservation are checked against the CPU implementation.
- Floating-point results are not bit-identical across processors. Tests allow small
  quantization differences and explicitly account for rare hard-sharpen-threshold
  and nearest-neighbor boundary differences. The 16-bit tests normally agree within
  eight levels; a sharpening cutoff can change a few boundary samples by up to one
  8-bit level while retaining 16-bit output precision.
- Blur/Heal brush samples now average premultiplied colors and ignore unrepresentable
  sub-byte coverage, avoiding transparent-color amplification. Histogram binning
  uses identical integer luminance weights on CPU and GPU.

## Reproducible checks

Run `scripts/check.sh --gpu` for formatting, clippy, CPU/application regressions,
native GPU comparisons, resource fallback, cancellation, worker-context propagation,
and the native Motion Blur Apply/Cancel/undo workflow.

For transfer-inclusive timings, run:

```sh
cargo test --release --locked --lib gpu::processing_tests::benchmark_processing_backends -- --ignored --nocapture --test-threads=1
```

On the RTX 3090 Vulkan adapter, a 1600×1200 synthetic image produced these median
warm-pipeline timings (three runs, release build). GPU measurements include
upload and final CPU readback; composition reuses its cached source texture.

| Operation | CPU | GPU | Speedup |
| --- | ---: | ---: | ---: |
| Gaussian Blur, radius 8 | 202 ms | 13.4 ms | 15× |
| Add Noise | 66.8 ms | 8.9 ms | 7.5× |
| Lens Correction | 112 ms | 9.1 ms | 12× |
| Exposure | 17.7 ms | 9.1 ms | 1.9× |
| Lanczos resize to 800×600 | 47.4 ms | 9.5 ms | 5.0× |
| Full-resolution composition | 16.4 ms | 7.2 ms | 2.3× |
| Luminance histogram | 2.6 ms | 6.0 ms | CPU retained |
| RAW development, default settings | 116 ms | 15.5 ms | 7.5× |

These are measurements for one workload and device, not a promise of real-time
performance for every image size, radius, layer count, or GPU.


## Resident RAW preview measurements

Release builds on an RTX 3090 (Vulkan, NVIDIA 610.57.04) and i9-10940X, using
Canon EOS M50 Mark II `IMG_5189.CR3` (4000×6000) and Nikon Z6 III `DSC_0144.NEF`
(4032×6048). New timings are medians of five warm runs with changing exposure.
Worker times include all development passes, histogram reduction, GPU completion,
and histogram readback. UI times cover native texture registration; neither
includes window presentation. Initial shader compilation and source upload are
excluded from warm runs.

| Stage | Canon CR3 | Nikon NEF |
| --- | ---: | ---: |
| Previous full-resolution render, including image transfers | 304 ms | 304 ms |
| Previous additional UI analysis and texture preparation | 261 ms | 255 ms |
| Resident full-resolution preview + histogram | 23.8 ms | 23.8 ms |
| Resident full-resolution preview + histogram + clipping texture | 27.9 ms | 28.9 ms |
| Native UI registration, without clipping | 0.041 ms | 0.039 ms |
| Resident 1600-pixel preview + histogram | 11.0 ms | 10.9 ms |
| Resident 3200-pixel preview + histogram | 13.3 ms | 13.7 ms |

The previous UI path always prepared clipping pixels. The warm preview improvements
do not eliminate initial decoding/demosaicing (about 1.0–1.1 seconds for these
samples), the first upload/allocation for a resolution, or full-resolution
Develop/TIFF materialization. Interactive previews use the small proxy and a
33 ms request interval; final detail follows 150 ms without edits. These timings
are stage measurements, not an end-to-end input-to-display latency guarantee.

Reproduce with `benchmark_raw_develop_preview` as documented in
[Development](DEVELOPMENT.md#raw-preview-performance). GPU regressions compare
resident textures against materialized output, histogram/clipping results, buffer
reuse, transparent pixels, row padding, and the immutability of previous frames.
