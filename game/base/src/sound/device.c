#define MA_NO_DECODING
#define MA_NO_ENCODING
#define MA_NO_GENERATION
#define MA_NO_RESOURCE_MANAGER
#define MA_NO_NODE_GRAPH
#define MA_NO_ENGINE
#define MINIAUDIO_IMPLEMENTATION
#include "miniaudio.h"

typedef void (*sound_device_callback)(void *user, float *out, int frames);

static ma_device g_device;
static ma_bool32 g_open = MA_FALSE;
static sound_device_callback g_callback = NULL;
static void *g_user = NULL;

static void sound_data(ma_device *device, void *output, const void *input, ma_uint32 frame_count)
{
    (void)device;
    (void)input;

    if (g_callback == NULL || output == NULL || frame_count == 0)
    {
        return;
    }

    g_callback(g_user, (float *)output, (int)frame_count);
}

int sound_device_start(
    sound_device_callback callback,
    void *user,
    int sample_rate,
    int channels,
    int period_frames)
{
    ma_device_config config;
    ma_result result;

    if (g_open)
    {
        ma_device_uninit(&g_device);
        g_open = MA_FALSE;
    }

    g_callback = callback;
    g_user = user;
    config = ma_device_config_init(ma_device_type_playback);
    config.playback.format = ma_format_f32;
    config.playback.channels = (ma_uint32)channels;
    config.sampleRate = (ma_uint32)sample_rate;
    config.periodSizeInFrames = (ma_uint32)period_frames;
    config.dataCallback = sound_data;
    config.performanceProfile = ma_performance_profile_low_latency;
    result = ma_device_init(NULL, &config, &g_device);

    if (result != MA_SUCCESS)
    {
        return 1;
    }

    result = ma_device_start(&g_device);

    if (result != MA_SUCCESS)
    {
        ma_device_uninit(&g_device);

        return 1;
    }

    g_open = MA_TRUE;

    return 0;
}

void sound_device_stop(void)
{
    if (!g_open)
    {
        return;
    }

    ma_device_uninit(&g_device);
    g_open = MA_FALSE;
    g_callback = NULL;
    g_user = NULL;
}
