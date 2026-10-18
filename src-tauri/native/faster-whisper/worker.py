"""Offline, persistent CTranslate2 worker. Audio is 16 kHz float32 PCM over stdin."""
import json
import os
from pathlib import Path
import sys

PREFIX = "ASMR_CT2:"

def emit(value):
    print(PREFIX + json.dumps(value, ensure_ascii=True), flush=True)

def read_exact(stream, count):
    data = bytearray()
    while len(data) < count:
        block = stream.read(count - len(data))
        if not block:
            raise EOFError("Incomplete PCM")
        data.extend(block)
    return data

def main():
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"
    runtime = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else Path(__file__).resolve().parent
    # Explicit installed CUDA path, never a shell or a model-provided executable.
    cuda = Path(sys.argv[1]) if len(sys.argv) > 1 else runtime
    handles = []
    if os.name == "nt" and cuda.is_dir():
        handles.append(os.add_dll_directory(str(cuda)))
        os.environ["PATH"] = str(cuda) + os.pathsep + os.environ.get("PATH", "")
    import numpy as np
    import ctranslate2
    from faster_whisper import WhisperModel
    model = None
    device = "CPU · INT8"
    # Probe by actual loading/inference: driver presence alone does not prove CUDA works.
    if cuda.joinpath("cublas64_12.dll").is_file() and cuda.joinpath("cudnn64_9.dll").is_file():
        try:
            if ctranslate2.get_cuda_device_count() > 0:
                model = WhisperModel(str(runtime / "model"), device="cuda", compute_type="int8_float16", local_files_only=True)
                list(model.transcribe(np.zeros(16000, dtype=np.float32), language="ja", beam_size=1)[0])
                device = "CUDA · INT8/FP16"
        except Exception:
            model = None
    if model is None:
        model = WhisperModel(str(runtime / "model"), device="cpu", compute_type="int8", cpu_threads=max(1,min(4,os.cpu_count() or 2)), local_files_only=True)
    emit({"device": device})
    stream = sys.stdin.buffer
    while True:
        header = stream.readline(32768)
        if not header:
            break
        if not header.endswith(b"\n"):
            raise ValueError("Invalid request")
        request = json.loads(header)
        count = request["count"]
        if not isinstance(count,int) or not 0 < count <= 320000:
            raise ValueError("Invalid PCM size")
        audio = np.frombuffer(read_exact(stream,count*4),dtype="<f4")
        if not np.isfinite(audio).all():
            raise ValueError("Invalid PCM")
        # Exact digital silence has no speech; avoid Whisper hallucinations without
        # imposing an energy threshold that would discard quiet ASMR syllables.
        if not np.any(audio):
            emit({"text":"","tokens":[],"timestamps":[],"segments":[],"event":""})
            continue
        language = request["language"]
        if language not in ("auto","ja","zh","en","ko"):
            raise ValueError("Invalid language")
        config = dict(task="transcribe", beam_size=5, temperature=[0.0, 0.2, 0.4],
            condition_on_previous_text=False, log_prob_threshold=-1.0,
            compression_ratio_threshold=2.4, no_speech_threshold=None,
            # The pinned distilled Kotoba model has incompatible alignment heads.
            # Keep decoder segment times; never call the native word aligner.
            word_timestamps=False, vad_filter=False,
            repetition_penalty=1.1, no_repeat_ngram_size=0)
        activity = None
        if request.get("speech_activity"):
            try:
                # Diagnostic only: never trim, reject or rescale quiet PCM.
                from faster_whisper.vad import get_vad_model
                probabilities = get_vad_model()(np.pad(audio, (0, (-len(audio)) % 512)))
                if probabilities.size and np.isfinite(probabilities).all():
                    activity = {"model": "silero-vad", "max_probability": float(probabilities.max()),
                        "mean_probability": float(probabilities.mean()), "frames": int(probabilities.size)}
            except Exception:
                # Missing optional runtime/asset is unavailable, not zero speech.
                activity = None
        segments, info = model.transcribe(audio, language=None if language=="auto" else language,
            initial_prompt=request.get("prompt") or None, **config)
        text, timed = [], []
        for segment in segments:
            text.append(segment.text)
            timed.append({"start":segment.start,"end":segment.end,"text":segment.text,
                "avg_logprob":segment.avg_logprob,"no_speech_prob":segment.no_speech_prob,
                "compression_ratio":segment.compression_ratio})
        emit({"text":"".join(text),"tokens":[],"timestamps":[],"segments":timed,"event":"",
            "detected_language":info.language,"config":config,"speech_activity":activity})

if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        emit({"error":"Faster-Whisper 运行失败："+str(exc)[:500]})
        sys.exit(1)
