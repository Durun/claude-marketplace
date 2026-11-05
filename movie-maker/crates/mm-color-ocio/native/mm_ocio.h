/* OCIO の C++ の API を、bindgen が読める C の口に畳む。例外は戻り値とエラー文字列に変える。 */
#ifndef MM_OCIO_H
#define MM_OCIO_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct MmOcioConfig MmOcioConfig;
typedef struct MmOcioProcessor MmOcioProcessor;

/* 直近の失敗の文章（スレッドごと）。次の呼び出しまで有効。 */
const char *mm_ocio_last_error(void);
const char *mm_ocio_version(void);

/* "ocio://..." なら組み込みの設定、それ以外は設定ファイルのパス。失敗は NULL。 */
MmOcioConfig *mm_ocio_config_create(const char *uri_or_path);
void mm_ocio_config_free(MmOcioConfig *config);
int mm_ocio_config_num_colorspaces(const MmOcioConfig *config);
const char *mm_ocio_config_colorspace_name(const MmOcioConfig *config, int index);
/* 0 なら存在しない。 */
int mm_ocio_config_has_colorspace(const MmOcioConfig *config, const char *name);

/* 既定の display と、その display の既定の view（src から見たもの）。NULL なら無い。 */
const char *mm_ocio_config_default_display(const MmOcioConfig *config);
const char *mm_ocio_config_default_view(const MmOcioConfig *config, const char *display, const char *src);

/* src を display と view で見るときの変換（トーンマップを含む）。display と view は NULL で既定。失敗は NULL。 */
MmOcioProcessor *mm_ocio_processor_create_display(const MmOcioConfig *config, const char *src,
                                                  const char *display, const char *view);

/* src から dst への変換。失敗は NULL。 */
MmOcioProcessor *mm_ocio_processor_create(const MmOcioConfig *config, const char *src, const char *dst);
void mm_ocio_processor_free(MmOcioProcessor *processor);
int mm_ocio_processor_is_noop(const MmOcioProcessor *processor);

/* CPU で rgb（3 個の float）をその場で変換する。成功で 0。 */
int mm_ocio_processor_apply_rgb(const MmOcioProcessor *processor, float *rgb);

/*
 * GPU のシェーダー（Vulkan 向けの GLSL 4.6）を作る。成功で 0。
 * text は processor を解放するまで有効。num_textures は 1D と 3D の LUT の数の合計、
 * num_uniforms は動的な値の数。function_name は text の中の入口の関数名。
 */
int mm_ocio_processor_gpu_shader(MmOcioProcessor *processor, const char *function_name,
                                 const char **text, int *num_textures, int *num_uniforms);

#ifdef __cplusplus
}
#endif

#endif
