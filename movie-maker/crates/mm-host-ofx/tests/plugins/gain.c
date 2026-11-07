/*
 * mm-host-ofx の試験に使う、最小の OpenFX のプラグイン（Filter、RGBA の float だけ）。
 * 出力 = 入力 x gain。パラメータの 4 つの型（Double、Integer、Boolean、Choice）と、可変長の
 * paramGetValueAtTime と message、標準出力への printf（応答の JSON に混ざらないこと）、
 * クラッシュ（crash）と遅い描画（delay_ms）を試すためのパラメータを持つ。
 */
#include <stdio.h>
#include <string.h>

#include "ofxImageEffect.h"
#include "ofxMessage.h"
#include "ofxPixels.h"

#ifdef _WIN32
#include <windows.h>
static void sleep_ms(int ms) { Sleep((DWORD)ms); }
#else
#include <unistd.h>
static void sleep_ms(int ms) { usleep((useconds_t)ms * 1000); }
#endif

static OfxHost *gHost;
static OfxImageEffectSuiteV1 *gEffect;
static OfxPropertySuiteV1 *gProp;
static OfxParameterSuiteV1 *gParam;
static OfxMessageSuiteV1 *gMessage;

static OfxStatus on_load(void) {
  if (!gHost) return kOfxStatErrMissingHostFeature;
  gEffect = (OfxImageEffectSuiteV1 *)gHost->fetchSuite(gHost->host, kOfxImageEffectSuite, 1);
  gProp = (OfxPropertySuiteV1 *)gHost->fetchSuite(gHost->host, kOfxPropertySuite, 1);
  gParam = (OfxParameterSuiteV1 *)gHost->fetchSuite(gHost->host, kOfxParameterSuite, 1);
  gMessage = (OfxMessageSuiteV1 *)gHost->fetchSuite(gHost->host, kOfxMessageSuite, 1);
  if (!gEffect || !gProp || !gParam || !gMessage) return kOfxStatErrMissingHostFeature;
  return kOfxStatOK;
}

static OfxStatus describe(OfxImageEffectHandle effect) {
  OfxPropertySetHandle props;
  gEffect->getPropertySet(effect, &props);
  gProp->propSetString(props, kOfxPropLabel, 0, "MM Test Gain");
  gProp->propSetString(props, kOfxImageEffectPluginPropGrouping, 0, "movie-maker/test");
  gProp->propSetString(props, kOfxPropPluginDescription, 0, "Multiplies RGB by gain");
  gProp->propSetString(props, kOfxImageEffectPropSupportedContexts, 0, kOfxImageEffectContextFilter);
  gProp->propSetString(props, kOfxImageEffectPropSupportedPixelDepths, 0, kOfxBitDepthFloat);
  printf("mm-test-gain: describe\n");
  fflush(stdout);
  return kOfxStatOK;
}

static void define_params(OfxParamSetHandle params) {
  OfxPropertySetHandle p;
  gParam->paramDefine(params, kOfxParamTypeDouble, "gain", &p);
  gProp->propSetString(p, kOfxPropLabel, 0, "Gain");
  gProp->propSetString(p, kOfxParamPropHint, 0, "RGB multiplier");
  gProp->propSetDouble(p, kOfxParamPropDefault, 0, 1.0);
  gProp->propSetDouble(p, kOfxParamPropMin, 0, 0.0);
  gProp->propSetDouble(p, kOfxParamPropMax, 0, 4.0);

  gParam->paramDefine(params, kOfxParamTypeChoice, "channel", &p);
  gProp->propSetString(p, kOfxParamPropChoiceOption, 0, "all");
  gProp->propSetString(p, kOfxParamPropChoiceOption, 1, "red");
  gProp->propSetInt(p, kOfxParamPropDefault, 0, 0);

  gParam->paramDefine(params, kOfxParamTypeInteger, "delay_ms", &p);
  gProp->propSetInt(p, kOfxParamPropDefault, 0, 0);
  gProp->propSetInt(p, kOfxParamPropMin, 0, 0);
  gProp->propSetInt(p, kOfxParamPropMax, 0, 60000);

  gParam->paramDefine(params, kOfxParamTypeBoolean, "crash", &p);
  gProp->propSetInt(p, kOfxParamPropDefault, 0, 0);
  gProp->propSetInt(p, kOfxParamPropSecret, 0, 1);
}

static OfxStatus describe_in_context(OfxImageEffectHandle effect) {
  OfxPropertySetHandle props;
  OfxParamSetHandle params;
  gEffect->clipDefine(effect, kOfxImageEffectOutputClipName, &props);
  gProp->propSetString(props, kOfxImageEffectPropSupportedComponents, 0, kOfxImageComponentRGBA);
  gEffect->clipDefine(effect, kOfxImageEffectSimpleSourceClipName, &props);
  gProp->propSetString(props, kOfxImageEffectPropSupportedComponents, 0, kOfxImageComponentRGBA);
  gEffect->getParamSet(effect, &params);
  define_params(params);
  return kOfxStatOK;
}

typedef struct Settings {
  double gain;
  int channel;
  int delay_ms;
  int crash;
} Settings;

static Settings read_settings(OfxImageEffectHandle instance, OfxTime time) {
  Settings s = {1.0, 0, 0, 0};
  OfxParamSetHandle params;
  OfxParamHandle h;
  gEffect->getParamSet(instance, &params);
  gParam->paramGetHandle(params, "gain", &h, NULL);
  gParam->paramGetValueAtTime(h, time, &s.gain);
  gParam->paramGetHandle(params, "channel", &h, NULL);
  gParam->paramGetValueAtTime(h, time, &s.channel);
  gParam->paramGetHandle(params, "delay_ms", &h, NULL);
  gParam->paramGetValueAtTime(h, time, &s.delay_ms);
  gParam->paramGetHandle(params, "crash", &h, NULL);
  gParam->paramGetValueAtTime(h, time, &s.crash);
  return s;
}

typedef struct Image {
  OfxPropertySetHandle handle;
  char *data;
  OfxRectI bounds;
  int row_bytes;
} Image;

static OfxStatus fetch(OfxImageEffectHandle instance, const char *clip, OfxTime time, Image *img) {
  OfxImageClipHandle c;
  gEffect->clipGetHandle(instance, clip, &c, NULL);
  if (gEffect->clipGetImage(c, time, NULL, &img->handle) != kOfxStatOK) return kOfxStatFailed;
  gProp->propGetPointer(img->handle, kOfxImagePropData, 0, (void **)&img->data);
  gProp->propGetIntN(img->handle, kOfxImagePropBounds, 4, &img->bounds.x1);
  gProp->propGetInt(img->handle, kOfxImagePropRowBytes, 0, &img->row_bytes);
  return kOfxStatOK;
}

static OfxRGBAColourF *pixel(const Image *img, int x, int y) {
  char *row = img->data + (size_t)(y - img->bounds.y1) * img->row_bytes;
  return (OfxRGBAColourF *)row + (x - img->bounds.x1);
}

static void process(const Image *src, const Image *dst, OfxRectI win, Settings s) {
  for (int y = win.y1; y < win.y2; ++y) {
    for (int x = win.x1; x < win.x2; ++x) {
      OfxRGBAColourF in = *pixel(src, x, y);
      OfxRGBAColourF *out = pixel(dst, x, y);
      out->r = (float)(in.r * s.gain);
      out->g = s.channel == 0 ? (float)(in.g * s.gain) : in.g;
      out->b = s.channel == 0 ? (float)(in.b * s.gain) : in.b;
      out->a = in.a;
    }
  }
}

static OfxStatus render(OfxImageEffectHandle instance, OfxPropertySetHandle in_args) {
  OfxTime time;
  OfxRectI win;
  Image src = {0}, dst = {0};
  gProp->propGetDouble(in_args, kOfxPropTime, 0, &time);
  gProp->propGetIntN(in_args, kOfxImageEffectPropRenderWindow, 4, &win.x1);
  Settings s = read_settings(instance, time);
  printf("mm-test-gain: render gain=%g\n", s.gain);
  fflush(stdout);
  gMessage->message(instance, kOfxMessageLog, NULL, "gain=%g channel=%d", s.gain, s.channel);
  if (s.crash) {
    *(volatile int *)0 = 1;
  }
  if (s.delay_ms > 0) sleep_ms(s.delay_ms);
  OfxStatus st = fetch(instance, kOfxImageEffectSimpleSourceClipName, time, &src);
  if (st == kOfxStatOK) st = fetch(instance, kOfxImageEffectOutputClipName, time, &dst);
  if (st == kOfxStatOK) process(&src, &dst, win, s);
  if (src.handle) gEffect->clipReleaseImage(src.handle);
  if (dst.handle) gEffect->clipReleaseImage(dst.handle);
  return st;
}

static OfxStatus plugin_main(const char *action, const void *handle, OfxPropertySetHandle in_args,
                             OfxPropertySetHandle out_args) {
  OfxImageEffectHandle effect = (OfxImageEffectHandle)handle;
  (void)out_args;
  if (strcmp(action, kOfxActionLoad) == 0) return on_load();
  if (strcmp(action, kOfxActionDescribe) == 0) return describe(effect);
  if (strcmp(action, kOfxImageEffectActionDescribeInContext) == 0) return describe_in_context(effect);
  if (strcmp(action, kOfxImageEffectActionRender) == 0) return render(effect, in_args);
  return kOfxStatReplyDefault;
}

static void set_host(OfxHost *host) { gHost = host; }

static OfxPlugin gPlugin = {
    kOfxImageEffectPluginApi, 1, "mm.test.Gain", 1, 2, set_host, plugin_main,
};

OfxPlugin *OfxGetPlugin(int nth) { return nth == 0 ? &gPlugin : NULL; }

int OfxGetNumberOfPlugins(void) { return 1; }
