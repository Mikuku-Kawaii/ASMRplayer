# Managed CTranslate2 recognition

The player explicitly installs two independent, offline model runtimes:

- `faster_whisper`: [Systran/faster-whisper-small](https://huggingface.co/Systran/faster-whisper-small), revision `536b0662742c02347bc0e980a01041f333bce120`.
- `kotoba_whisper`: [kotoba-tech/kotoba-whisper-v2.0-faster](https://huggingface.co/kotoba-tech/kotoba-whisper-v2.0-faster), revision `f44edd35eaeb2274e85ac7b31fb2c6f59ff1c4bc`. Japanese recognition only (explicit Japanese or automatic detection).

`assets.json` pins official Python 3.12.10 embedded Windows x64, PyPI wheels and model files by URL, byte length and SHA-256. The runtime uses faster-whisper 1.2.1 and CTranslate2 4.6.0. Setuptools stays at 80.9.0 because this CTranslate2 release imports `pkg_resources`. Installation extracts wheels into a private runtime without changing system Python or running pip. Model downloads happen only through the settings download buttons, with cancellation and staging before publication. Each model can be checked and deleted separately.

`worker.py` loads the model once per recognition task. Requests carry a JSON header and 16 kHz float32 PCM over stdin; prefixed JSON responses return text and decoder segment boundaries. It runs offline and does not download missing files. CPU uses INT8. When the player-managed SenseVoice FP16 CUDA directory is already installed, the worker probes CUDA with INT8/FP16 and otherwise falls back to CPU; the actual device is shown in task progress. CUDA inference is not yet covered by the recorded smoke tests.

Downloads use a 10-minute idle timeout with no total transfer deadline. Transport failures reconnect up to twice with validated HTTP Range resume. Failed or cancelled installations retain their private `downloads` cache; complete files are hash-checked before reuse. A server returning 200 to a Range request restarts that file instead of appending. Installation uses copies of cached inputs, so retrying a failed extraction does not download completed assets again; the cache is removed after publication. Ollama pull/create streams use a separate longer idle allowance because layer verification may not emit progress messages.

Word alignment is deliberately disabled: the pinned Kotoba model's supplied alignment heads cause a native access violation in this Windows runtime, reproduced with nonzero synthetic input. Decoder segment timestamps work. Rust decodes only the owned audio interval, clips segment boundaries to that interval, prevents overlap and applies existing short-phrase formatting. Text-based subdivision within a segment remains approximate, not forced alignment. Quiet nonzero audio is retained; exact digital silence bypasses decoding to prevent silence hallucinations.

Validation uses synthetic audio only. The ignored Rust test `t1513_ct2_real_worker_protocol_and_cancel` requires `ASMR_CT2_TEST_RUNTIME` pointing to an explicitly prepared isolated runtime. It checks two requests through a persistent worker, nonzero inference with bounded timestamps, language validation and cancellation. Model accuracy on real Japanese whispering and CUDA performance require separate acceptance testing.
