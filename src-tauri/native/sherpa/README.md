# SenseVoice native bridge

`c-api.h` and `LICENSE` are unmodified files from
https://github.com/k2-fsa/sherpa-onnx/tree/v1.12.20 (Apache-2.0).
`bridge.c` is the application's dynamically loaded CPU/CUDA adapter. It checks
the runtime version before constructing an ABI-dependent recognizer config.
No runtime or model is fetched during compilation, launch, or inference.

The bridge passes the user's validated language choice to SenseVoice. JSON event
metadata is retained for nonverbal-only segments: Breath, Laughter, Cough, Cry and
Sneeze become short sound words. Japanese recognition uses Japanese captions such
as ふう and ふふ; their Chinese translations remain in the translation layer.
Segment-level events have no word timing;
the owned audio is decoded separately before displaying them, so an event in the
overlapping context is not attributed to this segment. Speech text takes priority.
Event names and JSON fields follow upstream:
https://github.com/modelscope/FunASR/blob/main/funasr/utils/postprocess_utils.py
https://github.com/k2-fsa/sherpa-onnx/blob/v1.12.20/sherpa-onnx/csrc/offline-stream.cc

The adapter copies the official result JSON (text, tokens and timestamps) before
freeing the native result. Rust assigns overlapping context tokens to half-open
audio intervals and builds short subtitle cues from their timestamps. Missing or
inconsistent alignment falls back to recognition of the owned audio only.
Token timestamps are onsets, not sound endings. Explicit spaces between Japanese
phrases in the final model text now create boundaries, including when token text
omits those spaces; timestamps still come from tokens. A lone particle stays with
its continuation, and spaces inside Latin phrases do not split them. Unmarked
character gaps do not split words. Additional Japanese pause boundaries require
an explicit model word boundary, a 400 ms token gap and 240 ms of quiet PCM. Other tokens require
an 800 ms gap and 600 ms of quiet PCM. Model punctuation remains authoritative,
and Japanese text is no longer split solely at a character-count limit.
Nearby detected regions (gap at most 1.2 seconds) share a recognition window,
including intervening audio, up to twelve seconds. Audio segmentation uses
a lower continuation threshold after a detected onset so fading syllables stay
in the same model input; onset thresholds and the twelve-second input limit
remain unchanged. This does not reconstruct syllables the model has omitted.
The pinned SenseVoice conversion is documented in the upstream source:
https://github.com/k2-fsa/sherpa-onnx/blob/v1.12.20/sherpa-onnx/csrc/offline-recognizer-sense-voice-impl.h

## Optional FP16 / NVIDIA CUDA engine

The settings entry `SenseVoiceSmall · FP16 · GPU` uses the separate engine ID
`sensevoice_fp16`. Installation is explicit; selection, startup and recognition
never download dependencies. The existing INT8 CPU engine remains available.
The GPU engine requires Windows x64 and an NVIDIA CUDA-capable GPU/driver;
initialization failure is reported, without silently selecting CPU inference.

`sensevoice_fp16_assets.rs` pins the official FP32 ONNX model and tokens at
https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/tree/2365baeacb507f821a0c8120fcee3d484dba7a07
and sherpa-onnx 1.12.20's CUDA 12 / cuDNN 9 Windows archive. Additional Python
and NVIDIA wheel URLs, lengths and SHA-256 digests are in
`fp16-dependencies.json`. Python 3.12.10 is a private embedded conversion tool;
no system Python, pip environment, CUDA installation or PATH is changed.
Downloads total about 3.1 GB; allow 12 GB for staging and conversion. Verified
downloads survive cancellation and are reused. Only the complete model/runtime
directory is published, with an installation receipt.

`convert-fp16.py` converts the official floating-point weights locally using
Microsoft's unmodified `ort_float16.py` from ONNX Runtime v1.20.1:
https://github.com/microsoft/onnxruntime/blob/v1.20.1/onnxruntime/python/tools/transformers/float16.py
It preserves FP32 input/output types required by sherpa, model metadata and
tokens, sorts graph dependencies, and performs full ONNX type validation. The
converted model has 917 FP16 tensors and is 470,171,575 bytes. Unsupported
operations may still use FP32 or the ONNX CPU provider; CUDA is explicitly
required for the model's GPU execution path.

`onnxruntime_c_api.h` is unmodified from the same ONNX Runtime tag (MIT,
`ONNXRUNTIME-LICENSE`). `SENSEVOICE-LICENSE` is the upstream SenseVoice
Apache-2.0 license. Installation includes these and the sherpa/NVIDIA licenses.

GPU inference runs in an isolated instance of the player executable, selected
by `--sensevoice-gpu-worker` before Tauri initialization. This avoids CPU/GPU
ONNX DLL name collisions and contains native failures. The parent exchanges
bounded 16 kHz mono float PCM and result JSON over pipes, preserves token
timestamps, and kills/reaps the worker on cancellation, timeout or drop.
The explicit `检测 GPU` button loads the model and runs a synthetic silence
inference before reporting the device.

Manual validation uses `gpu-smoke-test.c` from a Visual Studio developer shell
with `/MD`, or the actual executable's worker protocol. The official model
repository's `test_wavs/ja.wav` was transcribed on an RTX 4060 Laptop GPU on
2026-10-16. The isolated full installer test can be run with a verified asset
cache in `ASMR_FP16_ASSET_CACHE` and the ignored test
`t1513_fp16_real_installer_from_verified_cache`. Never commit downloaded
weights, DLLs, test executables, recordings or temporary conversion artifacts.
