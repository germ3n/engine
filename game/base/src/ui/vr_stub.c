#include <stdint.h>

void vr_target_size(void *func, uint32_t *width, uint32_t *height)
{
    (void)func;

    if (width)
    {
        *width = 0;
    }

    if (height)
    {
        *height = 0;
    }
}

void vr_projection_raw(void *func, int32_t eye, float *left, float *right, float *top, float *bottom)
{
    (void)func;
    (void)eye;

    if (left)
    {
        *left = 0.0f;
    }

    if (right)
    {
        *right = 0.0f;
    }

    if (top)
    {
        *top = 0.0f;
    }

    if (bottom)
    {
        *bottom = 0.0f;
    }
}

void vr_eye_to_head(void *func, int32_t eye, float *matrix)
{
    (void)func;
    (void)eye;
    (void)matrix;
}

uint32_t vr_role_index(void *func, int32_t role)
{
    (void)func;
    (void)role;

    return 0;
}

int32_t vr_controller_axis(void *func, uint32_t index, float *x, float *y)
{
    (void)func;
    (void)index;

    if (x)
    {
        *x = 0.0f;
    }

    if (y)
    {
        *y = 0.0f;
    }

    return 0;
}

void vr_set_tracking_space(void *func, int32_t origin)
{
    (void)func;
    (void)origin;
}

int32_t vr_wait_hmd(void *func, float *matrix, int32_t *valid)
{
    (void)func;
    (void)matrix;

    if (valid)
    {
        *valid = 0;
    }

    return 1;
}

int32_t vr_submit(void *func, int32_t eye, void *handle, int32_t kind)
{
    (void)func;
    (void)eye;
    (void)handle;
    (void)kind;

    return 1;
}

int32_t vr_submit_d3d12(void *func, int32_t eye, void *resource, void *queue)
{
    (void)func;
    (void)eye;
    (void)resource;
    (void)queue;

    return 1;
}

void vr_handoff(void *func)
{
    (void)func;
}

uint32_t vr_vulkan_instance_extensions(void *func, int8_t *value, uint32_t size)
{
    (void)func;
    (void)value;
    (void)size;

    return 0;
}

uint32_t vr_vulkan_device_extensions(void *func, void *physical, int8_t *value, uint32_t size)
{
    (void)func;
    (void)physical;
    (void)value;
    (void)size;

    return 0;
}

uint32_t VR_InitInternal(int32_t *error, int32_t app_type)
{
    (void)app_type;

    if (error)
    {
        *error = 1;
    }

    return 0;
}

void VR_ShutdownInternal(void)
{
}

void *VR_GetGenericInterface(const int8_t *name, int32_t *error)
{
    (void)name;

    if (error)
    {
        *error = 1;
    }

    return 0;
}

uint8_t VR_IsHmdPresent(void)
{
    return 0;
}
