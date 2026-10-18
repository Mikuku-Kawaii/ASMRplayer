/* T1513: ABI compiled against the unmodified upstream v1.12.20 header.
 * Runtime DLLs are installed only through the explicit settings action. */
#include <windows.h>
#include <stdlib.h>
#include <string.h>
#include "c-api.h"
#include "onnxruntime_c_api.h"
#include <stdio.h>

typedef struct {
  HMODULE module;
  const SherpaOnnxOfflineRecognizer *recognizer;
  const SherpaOnnxOfflineRecognizer *(*create)(const SherpaOnnxOfflineRecognizerConfig *);
  void (*destroy)(const SherpaOnnxOfflineRecognizer *);
  const SherpaOnnxOfflineStream *(*stream)(const SherpaOnnxOfflineRecognizer *);
  void (*free_stream)(const SherpaOnnxOfflineStream *);
  void (*accept)(const SherpaOnnxOfflineStream *, int32_t, const float *, int32_t);
  void (*decode)(const SherpaOnnxOfflineRecognizer *, const SherpaOnnxOfflineStream *);
  const SherpaOnnxOfflineRecognizerResult *(*result)(const SherpaOnnxOfflineStream *);
  void (*free_result)(const SherpaOnnxOfflineRecognizerResult *);
} SenseVoice;

void asmr_sv_destroy(SenseVoice *s) {
  if (!s) return;
  if (s->recognizer) s->destroy(s->recognizer);
  if (s->module) FreeLibrary(s->module);
  free(s);
}

SenseVoice *asmr_sv_create(const wchar_t *dll, const char *model, const char *tokens, const char *language, const char *provider) {
  SenseVoice *s = (SenseVoice *)calloc(1, sizeof(SenseVoice));
  if (!s) return NULL;
  s->module = LoadLibraryExW(dll, NULL, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
  if (!s->module) { free(s); return NULL; }
  const char *(*version)(void) = (void *)GetProcAddress(s->module, "SherpaOnnxGetVersionStr");
  if (!version || strcmp(version(), "1.12.20") != 0) { asmr_sv_destroy(s); return NULL; }
#define LOAD(field, name) s->field = (void *)GetProcAddress(s->module, name); if (!s->field) { asmr_sv_destroy(s); return NULL; }
  LOAD(create, "SherpaOnnxCreateOfflineRecognizer")
  LOAD(destroy, "SherpaOnnxDestroyOfflineRecognizer")
  LOAD(stream, "SherpaOnnxCreateOfflineStream")
  LOAD(free_stream, "SherpaOnnxDestroyOfflineStream")
  LOAD(accept, "SherpaOnnxAcceptWaveformOffline")
  LOAD(decode, "SherpaOnnxDecodeOfflineStream")
  LOAD(result, "SherpaOnnxGetOfflineStreamResult")
  LOAD(free_result, "SherpaOnnxDestroyOfflineRecognizerResult")
#undef LOAD
  SherpaOnnxOfflineRecognizerConfig config = {0};
  config.feat_config.sample_rate = 16000;
  config.feat_config.feature_dim = 80;
  config.decoding_method = "greedy_search";
  config.model_config.num_threads = 2;
  config.model_config.provider = provider;
  config.model_config.tokens = tokens;
  config.model_config.sense_voice.model = model;
  config.model_config.sense_voice.language = language;
  config.model_config.sense_voice.use_itn = 1;
  s->recognizer = s->create(&config);
  if (!s->recognizer) { asmr_sv_destroy(s); return NULL; }
  return s;
}

char *asmr_sv_decode(SenseVoice *s, const float *samples, int32_t count) {
  const SherpaOnnxOfflineStream *stream = s->stream(s->recognizer);
  if (!stream) return NULL;
  s->accept(stream, 16000, samples, count);
  s->decode(s->recognizer, stream);
  const SherpaOnnxOfflineRecognizerResult *result = s->result(stream);
  /* Preserve token timestamps; the Rust side owns the copied JSON after stream cleanup. */
  char *text = result && result->json ? _strdup(result->json) : NULL;
  if (result) s->free_result(result);
  s->free_stream(stream);
  return text;
}
void asmr_sv_free_text(char *text) { free(text); }

/* Called only in the isolated GPU worker. Refuse CUDA failures instead of
 * allowing sherpa's unavailable-provider fallback to silently select CPU. */
int asmr_sv_cuda_probe(const wchar_t *runtime, char *message, size_t capacity) {
  SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
  AddDllDirectory(runtime);
  HMODULE driver = LoadLibraryExW(L"nvcuda.dll", NULL, LOAD_LIBRARY_SEARCH_SYSTEM32);
  if (!driver) { snprintf(message, capacity, "NVIDIA driver unavailable"); return 1; }
  int (__stdcall *init)(unsigned int) = (void *)GetProcAddress(driver, "cuInit");
  int (__stdcall *count)(int *) = (void *)GetProcAddress(driver, "cuDeviceGetCount");
  int (__stdcall *name)(char *, int, int) = (void *)GetProcAddress(driver, "cuDeviceGetName");
  int devices = 0;
  if (!init || !count || !name || init(0) != 0 || count(&devices) != 0 || devices < 1) {
    snprintf(message, capacity, "No usable NVIDIA CUDA device"); FreeLibrary(driver); return 2;
  }
  char device[256] = {0}; name(device, sizeof(device), 0);
  wchar_t path[32768];
  if (swprintf(path, 32768, L"%ls/onnxruntime.dll", runtime) < 0) { FreeLibrary(driver); return 3; }
  HMODULE ort = LoadLibraryExW(path, NULL, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
  const OrtApiBase *(ORT_API_CALL *base)(void) = ort ? (void *)GetProcAddress(ort, "OrtGetApiBase") : NULL;
  const OrtApi *api = base ? base()->GetApi(ORT_API_VERSION) : NULL;
  OrtStatus *(ORT_API_CALL *append)(OrtSessionOptions *, int) = ort ? (void *)GetProcAddress(ort, "OrtSessionOptionsAppendExecutionProvider_CUDA") : NULL;
  OrtSessionOptions *options = NULL;
  int result = 4;
  if (api && append) {
    OrtStatus *status = api->CreateSessionOptions(&options);
    if (!status) status = append(options, 0);
    if (status) {
      snprintf(message, capacity, "%s", api->GetErrorMessage(status));
      api->ReleaseStatus(status);
    } else {
      result = 0; snprintf(message, capacity, "%s", device);
    }
    if (options) api->ReleaseSessionOptions(options);
  } else { snprintf(message, capacity, "CUDA ONNX Runtime could not be loaded"); }
  /* This dedicated worker owns CUDA/ORT for its entire lifetime, including
   * both the probe and recognition. Process exit releases these references. */
  if (result != 0) {
    if (ort) FreeLibrary(ort);
    FreeLibrary(driver);
  }
  return result;
}
