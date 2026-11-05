#include "mm_ocio.h"

#include <OpenColorIO/OpenColorIO.h>

#include <string>

namespace OCIO = OCIO_NAMESPACE;

struct MmOcioConfig {
    OCIO::ConstConfigRcPtr config;
};

struct MmOcioProcessor {
    OCIO::ConstProcessorRcPtr processor;
    OCIO::ConstCPUProcessorRcPtr cpu;
    OCIO::GpuShaderDescRcPtr shader;
};

namespace {
thread_local std::string g_last_error;

void set_error(const char *what) { g_last_error = what ? what : "unknown"; }

bool starts_with(const std::string &s, const char *prefix) { return s.rfind(prefix, 0) == 0; }
}  // namespace

extern "C" {

const char *mm_ocio_last_error(void) { return g_last_error.c_str(); }

const char *mm_ocio_version(void) { return OCIO::GetVersion(); }

MmOcioConfig *mm_ocio_config_create(const char *uri_or_path) {
    try {
        std::string arg = uri_or_path ? uri_or_path : "";
        OCIO::ConstConfigRcPtr config = starts_with(arg, "ocio://")
                                            ? OCIO::Config::CreateFromBuiltinConfig(arg.c_str())
                                            : OCIO::Config::CreateFromFile(arg.c_str());
        return new MmOcioConfig{config};
    } catch (const std::exception &e) {
        set_error(e.what());
        return nullptr;
    }
}

void mm_ocio_config_free(MmOcioConfig *config) { delete config; }

int mm_ocio_config_num_colorspaces(const MmOcioConfig *config) {
    return config->config->getNumColorSpaces();
}

const char *mm_ocio_config_colorspace_name(const MmOcioConfig *config, int index) {
    return config->config->getColorSpaceNameByIndex(index);
}

int mm_ocio_config_has_colorspace(const MmOcioConfig *config, const char *name) {
    return config->config->getColorSpace(name) != nullptr ? 1 : 0;
}

const char *mm_ocio_config_default_display(const MmOcioConfig *config) {
    return config->config->getDefaultDisplay();
}

const char *mm_ocio_config_default_view(const MmOcioConfig *config, const char *display, const char *src) {
    return config->config->getDefaultView(display, src);
}

MmOcioProcessor *mm_ocio_processor_create_display(const MmOcioConfig *config, const char *src,
                                                  const char *display, const char *view) {
    try {
        const char *d = display ? display : config->config->getDefaultDisplay();
        const char *v = view ? view : config->config->getDefaultView(d, src);
        OCIO::DisplayViewTransformRcPtr t = OCIO::DisplayViewTransform::Create();
        t->setSrc(src);
        t->setDisplay(d);
        t->setView(v);
        auto *p = new MmOcioProcessor();
        p->processor = config->config->getProcessor(t);
        p->cpu = p->processor->getOptimizedCPUProcessor(OCIO::OPTIMIZATION_DEFAULT);
        return p;
    } catch (const std::exception &e) {
        set_error(e.what());
        return nullptr;
    }
}

MmOcioProcessor *mm_ocio_processor_create(const MmOcioConfig *config, const char *src, const char *dst) {
    try {
        auto *p = new MmOcioProcessor();
        p->processor = config->config->getProcessor(src, dst);
        p->cpu = p->processor->getOptimizedCPUProcessor(OCIO::OPTIMIZATION_DEFAULT);
        return p;
    } catch (const std::exception &e) {
        set_error(e.what());
        return nullptr;
    }
}

void mm_ocio_processor_free(MmOcioProcessor *processor) { delete processor; }

int mm_ocio_processor_is_noop(const MmOcioProcessor *processor) {
    return processor->processor->isNoOp() ? 1 : 0;
}

int mm_ocio_processor_apply_rgb(const MmOcioProcessor *processor, float *rgb) {
    try {
        processor->cpu->applyRGB(rgb);
        return 0;
    } catch (const std::exception &e) {
        set_error(e.what());
        return 1;
    }
}

int mm_ocio_processor_gpu_shader(MmOcioProcessor *processor, const char *function_name,
                                 const char **text, int *num_textures, int *num_uniforms) {
    try {
        OCIO::GpuShaderDescRcPtr desc = OCIO::GpuShaderDesc::CreateShaderDesc();
        desc->setLanguage(OCIO::GPU_LANGUAGE_GLSL_VK_4_6);
        desc->setFunctionName(function_name);
        desc->setResourcePrefix("ocio_");
        OCIO::ConstGPUProcessorRcPtr gpu = processor->processor->getDefaultGPUProcessor();
        gpu->extractGpuShaderInfo(desc);
        processor->shader = desc;
        *text = desc->getShaderText();
        *num_textures = static_cast<int>(desc->getNumTextures() + desc->getNum3DTextures());
        *num_uniforms = static_cast<int>(desc->getNumUniforms());
        return 0;
    } catch (const std::exception &e) {
        set_error(e.what());
        return 1;
    }
}

}  // extern "C"
