#include <stdint.h>
#include <string.h>
#include <stdbool.h>

#if defined(_WIN32)
#define VR_CALL __stdcall
#else
#define VR_CALL
#endif

typedef struct HmdMatrix34_t {
    float m[3][4];
} HmdMatrix34_t;

typedef struct HmdVector3_t {
    float v[3];
} HmdVector3_t;

typedef enum ETrackingResult {
    ETrackingResult_TrackingResult_Uninitialized = 1,
    ETrackingResult_TrackingResult_Calibrating_InProgress = 100,
    ETrackingResult_TrackingResult_Calibrating_OutOfRange = 101,
    ETrackingResult_TrackingResult_Running_OK = 200,
    ETrackingResult_TrackingResult_Running_OutOfRange = 201,
    ETrackingResult_TrackingResult_Fallback_RotationOnly = 300,
} ETrackingResult;

typedef struct TrackedDevicePose_t {
    HmdMatrix34_t mDeviceToAbsoluteTracking;
    HmdVector3_t vVelocity;
    HmdVector3_t vAngularVelocity;
    ETrackingResult eTrackingResult;
    bool bPoseIsValid;
    bool bDeviceIsConnected;
} TrackedDevicePose_t;

typedef struct VRControllerAxis_t {
    float x;
    float y;
} VRControllerAxis_t;

typedef struct VRControllerState_t {
    uint32_t unPacketNum;
    uint64_t ulButtonPressed;
    uint64_t ulButtonTouched;
    VRControllerAxis_t rAxis[5];
} VRControllerState_t;

typedef struct Texture_t {
    void *handle;
    int eType;
    int eColorSpace;
} Texture_t;

typedef struct D3D12TextureData_t {
    void *m_pResource;
    void *m_pCommandQueue;
    uint32_t m_nNodeMask;
} D3D12TextureData_t;

typedef void (VR_CALL *TargetSizeFn)(uint32_t *width, uint32_t *height);
typedef void (VR_CALL *ProjectionRawFn)(int eye, float *left, float *right, float *top, float *bottom);
typedef HmdMatrix34_t (VR_CALL *EyeToHeadFn)(int eye);
typedef uint32_t (VR_CALL *RoleIndexFn)(int role);
typedef bool (VR_CALL *ControllerStateFn)(uint32_t index, VRControllerState_t *state, uint32_t size);
typedef void (VR_CALL *SetSpaceFn)(int origin);
typedef int (VR_CALL *WaitPosesFn)(TrackedDevicePose_t *render, uint32_t render_count, TrackedDevicePose_t *game, uint32_t game_count);
typedef int (VR_CALL *SubmitFn)(int eye, Texture_t *texture, void *bounds, int flags);
typedef void (VR_CALL *HandoffFn)(void);

static void fn_copy(void *out, void *fn) {
    memcpy(out, &fn, sizeof(fn));
}

void vr_target_size(void *fn, uint32_t *width, uint32_t *height) {
    TargetSizeFn call;
    fn_copy(&call, fn);
    call(width, height);
}

void vr_projection_raw(void *fn, int eye, float *left, float *right, float *top, float *bottom) {
    ProjectionRawFn call;
    fn_copy(&call, fn);
    call(eye, left, right, top, bottom);
}

void vr_eye_to_head(void *fn, int eye, float *matrix) {
    EyeToHeadFn call;
    HmdMatrix34_t value;
    fn_copy(&call, fn);
    value = call(eye);
    memcpy(matrix, value.m, sizeof(value.m));
}

uint32_t vr_role_index(void *fn, int role) {
    RoleIndexFn call;
    fn_copy(&call, fn);

    return call(role);
}

int vr_controller_axis(void *fn, uint32_t index, float *x, float *y) {
    ControllerStateFn call;
    VRControllerState_t state;
    fn_copy(&call, fn);
    memset(&state, 0, sizeof(state));

    if (!call(index, &state, (uint32_t)sizeof(state))) {
        return 0;
    }

    *x = state.rAxis[0].x;
    *y = state.rAxis[0].y;

    return 1;
}

void vr_set_tracking_space(void *fn, int origin) {
    SetSpaceFn call;
    fn_copy(&call, fn);
    call(origin);
}

int vr_wait_hmd(void *fn, float *matrix, int *valid) {
    WaitPosesFn call;
    TrackedDevicePose_t render[64];
    TrackedDevicePose_t game[64];
    int err;
    fn_copy(&call, fn);
    memset(render, 0, sizeof(render));
    memset(game, 0, sizeof(game));
    err = call(render, 64, game, 64);
    *valid = render[0].bPoseIsValid ? 1 : 0;

    if (*valid) {
        memcpy(matrix, render[0].mDeviceToAbsoluteTracking.m, sizeof(render[0].mDeviceToAbsoluteTracking.m));
    }

    return err;
}

int vr_submit(void *fn, int eye, void *handle, int kind) {
    SubmitFn call;
    Texture_t texture;
    fn_copy(&call, fn);
    texture.handle = handle;
    texture.eType = kind;
    texture.eColorSpace = 0;

    return call(eye, &texture, 0, 0);
}

int vr_submit_d3d12(void *fn, int eye, void *resource, void *queue) {
    SubmitFn call;
    D3D12TextureData_t data;
    Texture_t texture;
    fn_copy(&call, fn);
    data.m_pResource = resource;
    data.m_pCommandQueue = queue;
    data.m_nNodeMask = 0;
    texture.handle = &data;
    texture.eType = 4;
    texture.eColorSpace = 0;

    return call(eye, &texture, 0, 0);
}

void vr_handoff(void *fn) {
    HandoffFn call;
    fn_copy(&call, fn);
    call();
}
