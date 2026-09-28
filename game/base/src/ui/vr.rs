use crate::ui::voxel::SceneView;
use std::ffi::c_void;

const TEX_D3D11: i32 = 0;
const TEX_GL: i32 = 1;
const TEX_SHARED: i32 = 5;
const TEX_METAL: i32 = 6;
const ROLE_LEFT: i32 = 1;
const ROLE_RIGHT: i32 = 2;
const SPACE_STANDING: i32 = 1;
const INVALID_DEVICE: u32 = u32::MAX;

#[derive(Clone, Copy)]
pub struct VrInput {
    pub active: bool,
    pub yaw: f32,
    pub move_x: f32,
    pub move_y: f32,
    pub turn: f32,
}

impl Default for VrInput {
    fn default() -> Self {
        Self {
            active: false,
            yaw: 0.0,
            move_x: 0.0,
            move_y: 0.0,
            turn: 0.0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct EyeViews {
    pub views: [SceneView; 2],
    pub width: u32,
    pub height: u32,
}

pub struct Headset {
    target_size: *mut c_void,
    projection_raw: *mut c_void,
    eye_to_head: *mut c_void,
    role_index: *mut c_void,
    controller: *mut c_void,
    wait_poses: *mut c_void,
    submit: *mut c_void,
    handoff_fn: *mut c_void,
    input: VrInput,
    anchor: Option<[f32; 3]>,
    submitted: bool,
    warned: bool,
}

pub fn connect(slot: &mut Option<Headset>, failed: &mut bool, enabled: bool, view: &SceneView) -> Option<EyeViews> {
    if !enabled {
        return None;
    }

    if slot.is_none() && !*failed {
        if !hmd_present() {
            return None;
        }

        match Headset::start() {
            Some(headset) => {
                println!("[vr] openvr");
                *slot = Some(headset);
            }
            None => {
                *failed = true;

                return None;
            }
        }
    }

    slot.as_mut()?.frame(view)
}

impl Headset {
    pub fn input(&self) -> VrInput {
        self.input
    }

    pub fn handoff(&mut self) {
        if !self.submitted {
            return;
        }

        self.submitted = false;
        unsafe { vr_handoff(self.handoff_fn) };
    }

    pub fn submit_gl(&mut self, eye: usize, name: u32) {
        self.finish_submit(eye, name as usize as *mut c_void, TEX_GL);
    }

    #[cfg(windows)]
    pub fn submit_d3d11(&mut self, eye: usize, texture: *mut c_void) {
        self.finish_submit(eye, texture, TEX_D3D11);
    }

    #[cfg(windows)]
    pub fn submit_d3d12(&mut self, eye: usize, resource: *mut c_void, queue: *mut c_void) {
        let err = unsafe { vr_submit_d3d12(self.submit, eye as i32, resource, queue) };
        self.note(err);
    }

    #[cfg(target_os = "macos")]
    pub fn submit_metal(&mut self, eye: usize, texture: *mut c_void) {
        self.finish_submit(eye, texture, TEX_METAL);
    }

    #[cfg(windows)]
    pub fn submit_shared(&mut self, eye: usize, handle: *mut c_void) {
        self.finish_submit(eye, handle, TEX_SHARED);
    }

    fn start() -> Option<Self> {
        let mut err = 0i32;
        let _token = unsafe { VR_InitInternal(&mut err, 1) };

        if err != 0 {
            println!("[vr] init {err}");

            return None;
        }

        let system = interface(b"FnTable:IVRSystem_026\0")?;
        let compositor = interface(b"FnTable:IVRCompositor_029\0")?;
        let target_size = required(slot(system, 0))?;
        let projection_raw = required(slot(system, 2))?;
        let eye_to_head = required(slot(system, 5))?;
        let role_index = required(slot(system, 18))?;
        let controller = required(slot(system, 37))?;
        let set_space = required(slot(compositor, 0))?;
        let wait_poses = required(slot(compositor, 2))?;
        let submit = required(slot(compositor, 6))?;
        let handoff_fn = required(slot(compositor, 9))?;
        unsafe { vr_set_tracking_space(set_space, SPACE_STANDING) };

        Some(Self {
            target_size,
            projection_raw,
            eye_to_head,
            role_index,
            controller,
            wait_poses,
            submit,
            handoff_fn,
            input: VrInput { active: true, ..VrInput::default() },
            anchor: None,
            submitted: false,
            warned: false,
        })
    }

    fn frame(&mut self, scene: &SceneView) -> Option<EyeViews> {
        self.input.active = true;
        self.input.move_x = 0.0;
        self.input.move_y = 0.0;
        self.input.turn = 0.0;
        self.sample_sticks();
        let mut head = [0.0; 12];
        let mut valid = 0i32;
        let err = unsafe { vr_wait_hmd(self.wait_poses, head.as_mut_ptr(), &mut valid) };

        if err != 0 || valid == 0 {
            return None;
        }

        let head = rows(&head);
        let head_pos = column(&head, 3);

        if self.anchor.is_none() {
            self.anchor = Some(head_pos);
        }

        let anchor = self.anchor.unwrap_or(head_pos);
        let yaw = planar_yaw(scene.forward);
        let head_forward = to_engine(neg(column(&head, 2)));
        self.input.yaw = planar_yaw(head_forward);
        let mut width = 0u32;
        let mut height = 0u32;
        unsafe { vr_target_size(self.target_size, &mut width, &mut height) };
        width = width.max(1);
        height = height.max(1);
        let mut views = [scene.clone(), scene.clone()];
        let mut idx = 0;

        while idx < 2 {
            views[idx] = self.eye_view(scene, &head, anchor, yaw, idx as i32);
            idx += 1;
        }

        Some(EyeViews { views, width, height })
    }

    fn eye_view(&self, scene: &SceneView, head: &[[f32; 4]; 3], anchor: [f32; 3], yaw: f32, eye: i32) -> SceneView {
        let mut eye_to_head = [0.0; 12];
        unsafe { vr_eye_to_head(self.eye_to_head, eye, eye_to_head.as_mut_ptr()) };
        let eye_m = mul34(*head, rows(&eye_to_head));
        let pos = column(&eye_m, 3);
        let delta = [pos[0] - anchor[0], pos[1] - anchor[1], pos[2] - anchor[2]];
        let delta = yaw_rotate(yaw, to_engine(delta));
        let scale = scene.scale.max(0.001);
        let mut left = 0.0;
        let mut right = 0.0;
        let mut top = 0.0;
        let mut bottom = 0.0;
        unsafe { vr_projection_raw(self.projection_raw, eye, &mut left, &mut right, &mut top, &mut bottom) };
        let forward = yaw_rotate(yaw, to_engine(neg(column(&eye_m, 2))));
        let up = yaw_rotate(yaw, to_engine(column(&eye_m, 1)));

        SceneView {
            eye: [
                scene.eye[0] + delta[0] * scale,
                scene.eye[1] + delta[1] * scale,
                scene.eye[2] + delta[2] * scale,
            ],
            forward,
            up,
            fov_y: scene.fov_y,
            aspect: scene.aspect,
            near: scene.near,
            far: scene.far,
            tangents: Some([left, right, top, bottom]),
            scale: scene.scale,
        }
    }

    fn sample_sticks(&mut self) {
        let left = self.stick(ROLE_LEFT);
        let right = self.stick(ROLE_RIGHT);
        self.input.move_x = deadzone(left[0]);
        self.input.move_y = deadzone(left[1]);
        self.input.turn = deadzone(right[0]);
    }

    fn stick(&self, role: i32) -> [f32; 2] {
        let index = unsafe { vr_role_index(self.role_index, role) };

        if index == INVALID_DEVICE {
            return [0.0, 0.0];
        }

        let mut x = 0.0;
        let mut y = 0.0;

        if unsafe { vr_controller_axis(self.controller, index, &mut x, &mut y) } == 0 {
            return [0.0, 0.0];
        }

        [x, y]
    }

    fn finish_submit(&mut self, eye: usize, handle: *mut c_void, kind: i32) {
        let err = unsafe { vr_submit(self.submit, eye as i32, handle, kind) };
        self.note(err);
    }

    fn note(&mut self, err: i32) {
        if err == 0 {
            self.submitted = true;

            return;
        }

        if self.warned {
            return;
        }

        self.warned = true;
        println!("[vr] submit {err}");
    }
}

impl Drop for Headset {
    fn drop(&mut self) {
        unsafe { VR_ShutdownInternal() };
    }
}

fn hmd_present() -> bool {
    unsafe { VR_IsHmdPresent() != 0 }
}

fn required(value: Option<*mut c_void>) -> Option<*mut c_void> {
    if value.is_none() {
        unsafe { VR_ShutdownInternal() };
    }

    value
}

fn interface(name: &[u8]) -> Option<*mut c_void> {
    let mut err = 0i32;
    let table = unsafe { VR_GetGenericInterface(name.as_ptr() as *const i8, &mut err) };

    if err != 0 || table.is_null() {
        println!("[vr] interface {err}");
        unsafe { VR_ShutdownInternal() };

        return None;
    }

    Some(table)
}

fn slot(table: *mut c_void, index: isize) -> Option<*mut c_void> {
    let fn_ptr = unsafe { *(table as *const *mut c_void).offset(index) };

    if fn_ptr.is_null() {
        return None;
    }

    Some(fn_ptr)
}

fn rows(values: &[f32; 12]) -> [[f32; 4]; 3] {
    [
        [values[0], values[1], values[2], values[3]],
        [values[4], values[5], values[6], values[7]],
        [values[8], values[9], values[10], values[11]],
    ]
}

fn column(matrix: &[[f32; 4]; 3], col: usize) -> [f32; 3] {
    [matrix[0][col], matrix[1][col], matrix[2][col]]
}

fn mul34(parent: [[f32; 4]; 3], child: [[f32; 4]; 3]) -> [[f32; 4]; 3] {
    let mut out = [[0.0; 4]; 3];
    let mut row = 0;

    while row < 3 {
        let mut col = 0;

        while col < 3 {
            out[row][col] = parent[row][0] * child[0][col] + parent[row][1] * child[1][col] + parent[row][2] * child[2][col];
            col += 1;
        }

        out[row][3] = parent[row][0] * child[0][3] + parent[row][1] * child[1][3] + parent[row][2] * child[2][3] + parent[row][3];
        row += 1;
    }

    out
}

fn to_engine(value: [f32; 3]) -> [f32; 3] {
    [-value[2], -value[0], value[1]]
}

fn neg(value: [f32; 3]) -> [f32; 3] {
    [-value[0], -value[1], -value[2]]
}

fn yaw_rotate(yaw: f32, value: [f32; 3]) -> [f32; 3] {
    let c = yaw.cos();
    let s = yaw.sin();

    [c * value[0] - s * value[1], s * value[0] + c * value[1], value[2]]
}

fn planar_yaw(forward: [f32; 3]) -> f32 {
    forward[1].atan2(forward[0])
}

fn deadzone(value: f32) -> f32 {
    if value.abs() < 0.15 {
        return 0.0;
    }

    value
}

pub fn view_proj(view: &SceneView, zero_to_one: bool) -> [f32; 16] {
    let view_matrix = look_forward(view.eye, view.forward, view.up);
    let mut proj = projection(view);

    if zero_to_one {
        let mut col = 0;

        while col < 4 {
            let z = proj[col * 4 + 2];
            let w = proj[col * 4 + 3];
            proj[col * 4 + 2] = z * 0.5 + w * 0.5;
            col += 1;
        }
    }

    mul(proj, view_matrix)
}

fn projection(view: &SceneView) -> [f32; 16] {
    let near = view.near;
    let far = view.far;
    let (left, right, top, bottom) = match view.tangents {
        Some(tan) => (tan[0] * near, tan[1] * near, tan[2] * near, tan[3] * near),
        None => {
            let top = (view.fov_y * 0.5).tan() * near;
            let right = top * view.aspect.max(0.01);

            (-right, right, top, -top)
        }
    };
    let width = (right - left).abs().max(1e-6);
    let height = (top - bottom).abs().max(1e-6);
    let nf = 1.0 / (near - far);

    [
        2.0 * near / width, 0.0, 0.0, 0.0,
        0.0, 2.0 * near / height, 0.0, 0.0,
        (right + left) / width, (top + bottom) / height, (far + near) * nf, -1.0,
        0.0, 0.0, (2.0 * far * near) * nf, 0.0,
    ]
}

fn look_forward(eye: [f32; 3], forward: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let f = normalize(forward);
    let zaxis = [-f[0], -f[1], -f[2]];
    let xaxis = normalize(cross(up, zaxis));
    let yaxis = cross(zaxis, xaxis);

    [
        xaxis[0], yaxis[0], zaxis[0], 0.0,
        xaxis[1], yaxis[1], zaxis[1], 0.0,
        xaxis[2], yaxis[2], zaxis[2], 0.0,
        -dot(xaxis, eye), -dot(yaxis, eye), -dot(zaxis, eye), 1.0,
    ]
}

fn mul(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut out = [0.0; 16];
    let mut col = 0;

    while col < 4 {
        let mut row = 0;

        while row < 4 {
            let mut sum = 0.0;
            let mut idx = 0;

            while idx < 4 {
                sum += a[idx * 4 + row] * b[col * 4 + idx];
                idx += 1;
            }

            out[col * 4 + row] = sum;
            row += 1;
        }

        col += 1;
    }

    out
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt();

    if len <= 0.0 {
        return [0.0, 0.0, 0.0];
    }

    [v[0] / len, v[1] / len, v[2] / len]
}

extern "C" {
    fn vr_target_size(func: *mut c_void, width: *mut u32, height: *mut u32);
    fn vr_projection_raw(func: *mut c_void, eye: i32, left: *mut f32, right: *mut f32, top: *mut f32, bottom: *mut f32);
    fn vr_eye_to_head(func: *mut c_void, eye: i32, matrix: *mut f32);
    fn vr_role_index(func: *mut c_void, role: i32) -> u32;
    fn vr_controller_axis(func: *mut c_void, index: u32, x: *mut f32, y: *mut f32) -> i32;
    fn vr_set_tracking_space(func: *mut c_void, origin: i32);
    fn vr_wait_hmd(func: *mut c_void, matrix: *mut f32, valid: *mut i32) -> i32;
    fn vr_submit(func: *mut c_void, eye: i32, handle: *mut c_void, kind: i32) -> i32;
    fn vr_submit_d3d12(func: *mut c_void, eye: i32, resource: *mut c_void, queue: *mut c_void) -> i32;
    fn vr_handoff(func: *mut c_void);
    fn VR_InitInternal(error: *mut i32, app_type: i32) -> u32;
    fn VR_ShutdownInternal();
    fn VR_GetGenericInterface(name: *const i8, error: *mut i32) -> *mut c_void;
    fn VR_IsHmdPresent() -> u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> SceneView {
        SceneView {
            eye: [0.0, 0.0, 0.0],
            forward: [1.0, 0.0, 0.0],
            up: [0.0, 0.0, 1.0],
            fov_y: 90.0_f32.to_radians(),
            aspect: 1.0,
            near: 1.0,
            far: 100.0,
            tangents: None,
            scale: 1.0,
        }
    }

    #[test]
    fn symmetric_frustum_is_centered() {
        let proj = projection(&view());

        assert!(proj[8].abs() < 1e-5);
        assert!(proj[9].abs() < 1e-5);
    }

    #[test]
    fn shifted_frustum_moves_the_center() {
        let mut scene = view();
        scene.tangents = Some([-0.5, 1.5, 1.0, -1.0]);
        let proj = projection(&scene);

        assert!(proj[8] > 0.2);
    }

    #[test]
    fn tracking_up_becomes_engine_up() {
        let up = to_engine([0.0, 1.0, 0.0]);

        assert!((up[2] - 1.0).abs() < 1e-5);
    }
}
