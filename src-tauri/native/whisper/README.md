# Whisper recognition bridge

Unmodified headers and MIT license from [whisper.cpp v1.8.2](https://github.com/ggml-org/whisper.cpp/tree/v1.8.2).
`bridge.c` is the application's Windows dynamic loading adapter. It checks the runtime version before using the pinned C ABI, loads models with Unicode Windows paths, keeps one context per recognition job, and passes bounded glossary source terms to `whisper_full_params.initial_prompt`.

No model or DLL is included. The settings download action installs the official CPU x64 runtime and multilingual small Q5_1 model only after a user click, with pinned size and SHA-256 verification. Runtime/model source metadata is recorded in `src/services/whisper_assets.rs`.

Whisper timestamps are not used as word alignment here; existing owned-audio segmentation and punctuation-free short phrases remain authoritative. Whisper does not supply SenseVoice sound-event labels.

Recognition groups nearby detected regions with gaps up to 1.2 seconds into an audio window bounded to 12 seconds, retaining the intervening audio. Japanese subtitle boundaries use model punctuation and explicit spaces between Japanese phrases, without arbitrary character-count cuts; lone particles stay with their continuation and Latin phrases retain internal spaces. Whisper uses deterministic beam search (width 5, temperature 0) instead of greedy decoding to compare alternative transcriptions. This costs more CPU time and does not guarantee correct homophones; no transcript-wide word substitution is applied.
