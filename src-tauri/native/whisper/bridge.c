/* ABI pinned to whisper.cpp v1.8.2. No model or runtime is bundled/downloaded here. */
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "whisper.h"

typedef struct {
  HMODULE module;
  struct whisper_context *context;
  void (*free_context)(struct whisper_context *);
  struct whisper_full_params (*params)(enum whisper_sampling_strategy);
  int (*full)(struct whisper_context *, struct whisper_full_params, const float *, int);
  int (*segments)(struct whisper_context *);
  const char *(*text)(struct whisper_context *, int);
  int64_t (*t0)(struct whisper_context *, int);
  int64_t (*t1)(struct whisper_context *, int);
  float (*no_speech)(struct whisper_context *, int);
  int (*tokenize)(struct whisper_context *, const char *, whisper_token *, int);
  char *last_prompt;
} AsmrWhisper;

void asmr_whisper_destroy(AsmrWhisper *s) {
  if (!s) return;
  if (s->context) s->free_context(s->context);
  if (s->module) FreeLibrary(s->module);
  free(s->last_prompt);
  free(s);
}
static size_t model_read(void *ctx, void *out, size_t size) { return fread(out, 1, size, *(FILE **)ctx); }
static bool model_eof(void *ctx) { return feof(*(FILE **)ctx) != 0; }
static void model_close(void *ctx) { if (*(FILE **)ctx) { fclose(*(FILE **)ctx); *(FILE **)ctx = NULL; } }
static void quiet_log(enum ggml_log_level level, const char *text, void *data) { (void)level; (void)text; (void)data; }

AsmrWhisper *asmr_whisper_create(const wchar_t *dll, const wchar_t *model) {
  AsmrWhisper *s = calloc(1, sizeof(*s));
  if (!s) return NULL;
  s->module = LoadLibraryExW(dll, NULL, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
  if (!s->module) { free(s); return NULL; }
  const char *(*version)(void) = (void *)GetProcAddress(s->module, "whisper_version");
  if (!version || strcmp(version(), "1.8.2") != 0) { asmr_whisper_destroy(s); return NULL; }
#define LOAD(field, name) s->field = (void *)GetProcAddress(s->module, name); if (!s->field) { asmr_whisper_destroy(s); return NULL; }
  LOAD(free_context, "whisper_free")
  LOAD(params, "whisper_full_default_params")
  LOAD(full, "whisper_full")
  LOAD(segments, "whisper_full_n_segments")
  LOAD(text, "whisper_full_get_segment_text")
  LOAD(t0, "whisper_full_get_segment_t0")
  LOAD(t1, "whisper_full_get_segment_t1")
  LOAD(no_speech, "whisper_full_get_segment_no_speech_prob")
  LOAD(tokenize, "whisper_tokenize")
#undef LOAD
  struct whisper_context_params (*defaults)(void) = (void *)GetProcAddress(s->module, "whisper_context_default_params");
  struct whisper_context *(*init)(struct whisper_model_loader *, struct whisper_context_params) = (void *)GetProcAddress(s->module, "whisper_init_with_params");
  void (*log_set)(ggml_log_callback, void *) = (void *)GetProcAddress(s->module, "whisper_log_set");
  if (!defaults || !init || !log_set) { asmr_whisper_destroy(s); return NULL; }
  log_set(quiet_log, NULL);
  struct whisper_context_params config = defaults();
  config.use_gpu = false;
  FILE *file = _wfopen(model, L"rb");
  if (!file) { asmr_whisper_destroy(s); return NULL; }
  struct whisper_model_loader loader = { &file, model_read, model_eof, model_close };
  s->context = init(&loader, config);
  model_close(&file);
  if (!s->context) { asmr_whisper_destroy(s); return NULL; }
  return s;
}

int asmr_whisper_decode(AsmrWhisper *s, const float *samples, int count,
    const char *language, const char *prompt, bool (*cancel)(void *), void *cancel_data) {
  struct whisper_full_params config = s->params(WHISPER_SAMPLING_BEAM_SEARCH);
  config.beam_search.beam_size = 5;
  config.beam_search.patience = -1.0f;
  config.temperature = 0.0f;
  config.n_threads = 2;
  config.no_context = true; /* No transcript or glossary leakage between audio windows. */
  config.translate = false;
  config.language = language;
  config.print_progress = config.print_realtime = config.print_timestamps = config.print_special = false;
  config.abort_callback = cancel;
  config.abort_callback_user_data = cancel_data;
  config.temperature_inc = 0.0f;
  config.suppress_nst = false; /* Preserve quiet hums and audible non-speech tokens. */
  config.no_speech_thold = 1.0f; /* Audit rather than silently discard low-energy results. */
  /* Retain whole terms in priority order, within the model's 224-token prompt limit. */
  char *bounded = _strdup(prompt);
  if (!bounded) return -1;
  whisper_token tokens[224];
  while (bounded[0] && s->tokenize(s->context, bounded, tokens, 224) < 0) {
    char *end = strrchr(bounded, '\n');
    if (end) *end = '\0'; else bounded[0] = '\0';
  }
  config.initial_prompt = bounded[0] ? bounded : NULL;
  config.n_max_text_ctx = 224;
  int result = s->full(s->context, config, samples, count);
  free(s->last_prompt);
  s->last_prompt = bounded;
  return result;
}
int asmr_whisper_segments(AsmrWhisper *s) { return s->segments(s->context); }
const char *asmr_whisper_text(AsmrWhisper *s, int index) { return s->text(s->context, index); }
int64_t asmr_whisper_t0(AsmrWhisper *s, int index) { return s->t0(s->context, index); }
int64_t asmr_whisper_t1(AsmrWhisper *s, int index) { return s->t1(s->context, index); }
float asmr_whisper_no_speech(AsmrWhisper *s, int index) { return s->no_speech(s->context, index); }
const char *asmr_whisper_prompt(AsmrWhisper *s) { return s->last_prompt ? s->last_prompt : ""; }
