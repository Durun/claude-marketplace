/*
 * OpenFX の可変長引数の関数を受け、Rust の固定長の関数（src/host/ の mm_ofx_*）へ渡す。
 * Rust の安定版では可変長引数の関数を定義できないため、ここだけ C で書く。
 * あわせて、プロセスの準備（標準出力の付け替え、クラッシュのダイアログの抑止）と、
 * 公式のヘッダーの構造体の大きさ（Rust 側の repr(C) と試験で突き合わせる）を出す。
 */
#include <stdarg.h>
#include <stdio.h>

#include "ofxCore.h"
#include "ofxImageEffect.h"
#include "ofxMemory.h"
#include "ofxMessage.h"
#include "ofxMultiThread.h"
#include "ofxParam.h"
#include "ofxProperty.h"

#ifdef _WIN32
#include <io.h>
#include <windows.h>
#else
#include <unistd.h>
#endif

/* パラメータの値の成分の型。Rust の host::params::Shape と同じ値。 */
enum { MM_SHAPE_NONE = 0, MM_SHAPE_INT = 1, MM_SHAPE_DOUBLE = 2, MM_SHAPE_STRING = 3 };
/* 成分の数の上限（RGBA の 4）。 */
#define MM_MAX_DIM 4
/* message の書式を展開する先の大きさ。超えた分は切り捨てる。 */
#define MM_MESSAGE_LEN 2048

int mm_ofx_param_shape(OfxParamHandle h, int *count);
OfxStatus mm_ofx_param_get(OfxParamHandle h, double scale, void **out, int count);
OfxStatus mm_ofx_param_set_ints(OfxParamHandle h, const int *v, int count);
OfxStatus mm_ofx_param_set_doubles(OfxParamHandle h, const double *v, int count);
OfxStatus mm_ofx_param_set_string(OfxParamHandle h, const char *v);
OfxStatus mm_ofx_message(void *handle, const char *type, const char *id, const char *text);

static OfxStatus collect_out(OfxParamHandle h, double scale, va_list ap) {
  int count = 0;
  int shape = mm_ofx_param_shape(h, &count);
  void *out[MM_MAX_DIM] = {0};
  if (shape == MM_SHAPE_NONE || count > MM_MAX_DIM) {
    return kOfxStatErrBadHandle;
  }
  for (int i = 0; i < count; ++i) {
    out[i] = va_arg(ap, void *);
  }
  return mm_ofx_param_get(h, scale, out, count);
}

static OfxStatus apply_in(OfxParamHandle h, va_list ap) {
  int count = 0;
  int shape = mm_ofx_param_shape(h, &count);
  if (count > MM_MAX_DIM) {
    return kOfxStatErrBadHandle;
  }
  if (shape == MM_SHAPE_INT) {
    int v[MM_MAX_DIM] = {0};
    for (int i = 0; i < count; ++i) v[i] = va_arg(ap, int);
    return mm_ofx_param_set_ints(h, v, count);
  }
  if (shape == MM_SHAPE_DOUBLE) {
    double v[MM_MAX_DIM] = {0};
    for (int i = 0; i < count; ++i) v[i] = va_arg(ap, double);
    return mm_ofx_param_set_doubles(h, v, count);
  }
  if (shape == MM_SHAPE_STRING) {
    return mm_ofx_param_set_string(h, va_arg(ap, const char *));
  }
  return kOfxStatErrBadHandle;
}

/* ホストは値を時刻ごとに持たない（アプリが評価した値を毎フレーム渡す）ので、時刻は見ない。 */
OfxStatus mm_ofx_param_get_value(OfxParamHandle h, ...) {
  va_list ap;
  va_start(ap, h);
  OfxStatus s = collect_out(h, 1.0, ap);
  va_end(ap);
  return s;
}

OfxStatus mm_ofx_param_get_value_at_time(OfxParamHandle h, OfxTime time, ...) {
  va_list ap;
  va_start(ap, time);
  OfxStatus s = collect_out(h, 1.0, ap);
  va_end(ap);
  return s;
}

OfxStatus mm_ofx_param_get_derivative(OfxParamHandle h, OfxTime time, ...) {
  va_list ap;
  va_start(ap, time);
  OfxStatus s = collect_out(h, 0.0, ap);
  va_end(ap);
  return s;
}

OfxStatus mm_ofx_param_get_integral(OfxParamHandle h, OfxTime t1, OfxTime t2, ...) {
  va_list ap;
  va_start(ap, t2);
  OfxStatus s = collect_out(h, t2 - t1, ap);
  va_end(ap);
  return s;
}

OfxStatus mm_ofx_param_set_value(OfxParamHandle h, ...) {
  va_list ap;
  va_start(ap, h);
  OfxStatus s = apply_in(h, ap);
  va_end(ap);
  return s;
}

OfxStatus mm_ofx_param_set_value_at_time(OfxParamHandle h, OfxTime time, ...) {
  va_list ap;
  va_start(ap, time);
  OfxStatus s = apply_in(h, ap);
  va_end(ap);
  return s;
}

OfxStatus mm_ofx_message_v(void *handle, const char *type, const char *id, const char *format, ...) {
  char text[MM_MESSAGE_LEN];
  va_list ap;
  va_start(ap, format);
  vsnprintf(text, sizeof text, format ? format : "", ap);
  va_end(ap);
  return mm_ofx_message(handle, type, id, text);
}

/* プラグインが printf した行が応答の JSON に混ざらないよう、標準出力を標準エラーへ付け替える。
   応答は付け替える前に複製しておいた標準出力へ書く。 */
int mm_ofx_stdout_to_stderr(void) {
  fflush(stdout);
#ifdef _WIN32
  SetStdHandle(STD_OUTPUT_HANDLE, GetStdHandle(STD_ERROR_HANDLE));
  return _dup2(_fileno(stderr), _fileno(stdout));
#else
  return dup2(STDERR_FILENO, STDOUT_FILENO);
#endif
}

/* プラグインのクラッシュでエラー報告のダイアログを出さず、すぐ終わってアプリに知らせる。 */
void mm_ofx_quiet_crash_dialogs(void) {
#ifdef _WIN32
  SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
#endif
}

/* 公式のヘッダーの構造体の大きさ。番号は Rust の host::ffi::Layout と同じ。 */
size_t mm_ofx_layout(int which) {
  switch (which) {
  case 0: return sizeof(OfxHost);
  case 1: return sizeof(OfxPlugin);
  case 2: return sizeof(OfxPropertySuiteV1);
  case 3: return sizeof(OfxParameterSuiteV1);
  case 4: return sizeof(OfxImageEffectSuiteV1);
  case 5: return sizeof(OfxMemorySuiteV1);
  case 6: return sizeof(OfxMultiThreadSuiteV1);
  case 7: return sizeof(OfxMessageSuiteV2);
  case 8: return sizeof(OfxRectI);
  case 9: return sizeof(OfxRectD);
  default: return 0;
  }
}
