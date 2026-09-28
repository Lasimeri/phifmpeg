# 2026-09-27: where a card spends an x265 encode

Card 0, one 120-frame segment of the Big Buck Bunny 1080p60 excerpt
(segment 0 of the transcode input), encoded alone with the card slot's
settings: FFmpeg n9.0.2 C build, `-threads 2`, libx265 ultrafast CRF 28,
`pools=28:frame-threads=2`. Profiled with the `phix` sampler
(`PHIX_PROF=/data/phifmpeg/t/x265.prof`, 10 ms of process CPU time per
sample) and mapped on the host with `phifmpeg prof build/build/c/ffmpeg_g
x265-card.prof`. The binary on the card is the stripped copy of that
`ffmpeg_g` (same SHA-256 as `build/build/c/ffmpeg`, `84f3393f4897b447...`);
stripping does not move code, so the addresses map directly.

Run: 568.0 s CPU, 41.8 s wall. 31,443 samples, 1 outside any symbol.
Fewer samples than 568 s / 10 ms would give (about one per 18 ms of CPU):
the process-wide timer seems to deliver at most one signal per check, so
some expiries coalesce. That is not measured further here; the shares
below assume the loss does not favour any function.

## By function (top 16, 84.96 percent of samples)

| share | cumulative | function (x265 C, 8-bit) |
| --- | --- | --- |
| 18.25% | 18.25% | `satd_8x4` |
| 15.31% | 33.56% | `intra_pred_ang_c<8>` |
| 10.16% | 43.71% | `sad_x4<8, 8>` |
| 7.42% | 51.13% | `sad_x3<8, 8>` |
| 7.20% | 58.33% | `sad_x4<32, 32>` |
| 4.81% | 63.14% | `frame_init_lowres_core` |
| 3.93% | 67.07% | `_sa8d_8x8` |
| 3.28% | 70.36% | `interp_horiz_pp_c<8, 32, 32>` |
| 3.12% | 73.47% | `interp_vert_pp_c<8, 32, 32>` |
| 2.02% | 75.49% | `interp_hv_pp_c<8, 32, 32>` |
| 2.01% | 77.50% | `sad<8, 8>` |
| 1.76% | 79.26% | `intra_pred_ang_c<32>` |
| 1.63% | 80.90% | `CostEstimateGroup::estimateCUCost` |
| 1.53% | 82.43% | `pixelavg_pp<8, 8>` |
| 1.35% | 83.78% | `MotionEstimate::motionEstimate` |
| 1.18% | 84.96% | `sad<32, 32>` |

(The next four, `psyCost_pp<3>`, `planar_pred_c<3>`, `sse<32, 32>` and
`partialButterfly32`, are 1.18, 1.17, 1.00 and 0.93 percent.)

## By family

| family | share | functions |
| --- | --- | --- |
| sum of absolute differences (motion search) | 28.0% | `sad_x4`, `sad_x3`, `sad` at 8x8 and 32x32 |
| Hadamard costs (mode decision) | 22.8% | `satd_8x4`, `_sa8d_8x8`, `satd8` |
| intra prediction | 18.9% | `intra_pred_ang` 8 and 32, `planar_pred`, `intraFilter`, `intra_pred_dc` |
| sub-pixel interpolation | 8.4% | `interp_{horiz,vert,hv}_pp` 32x32 |
| lookahead | 5.6% | `frame_init_lowres_core`, `lowresIntraEstimate` |
| transforms and quantisation | 2.5% | `partialButterfly*`, `idct_32x32`, `quant_c` |

## What it means

About a dozen small pixel functions take roughly 80 percent of the card's
encode time; the encoder's control logic is a thin remainder. Each of them
is an x265 primitive with a plain C reference inside x265, which is what
any faster version must match exactly. On the host, x265's own SIMD
versions of these functions make it 3.0x faster than its C (SSE2) and 6.5x
(AVX2) on one thread ([encode baseline](2026-09-27-encode-baseline.md)).
