#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::voxel::FlyCamera;
    use crate::world::{Block, BlockPos, VoxelWorld};

    /*#[test]
    fn d3d_projection_shows_front_faces() {
        let mut world = VoxelWorld::new();
        world.set(BlockPos::new(0, 0, 0), Block(1));
        let mesh = world.mesh();
        let view = FlyCamera::new().scene(4.0 / 3.0, 1.0);
        let view_proj = view_proj(&view);
        let mut visible = 0;
        let mut front = 0;
        let mut idx = 0;

        while idx + crate::world::STRIDE <= mesh.len() {
            let a = project(&view_proj, [mesh[idx], mesh[idx + 1], mesh[idx + 2]]);
            let b = project(&view_proj, [mesh[idx + 6], mesh[idx + 7], mesh[idx + 8]]);
            let c = project(&view_proj, [mesh[idx + 12], mesh[idx + 13], mesh[idx + 14]]);

            if a[3] > 0.0 && b[3] > 0.0 && c[3] > 0.0 {
                let ax = a[0] / a[3];
                let ay = a[1] / a[3];
                let az = a[2] / a[3];
                let bx = b[0] / b[3];
                let by = b[1] / b[3];
                let bz = b[2] / b[3];
                let cx = c[0] / c[3];
                let cy = c[1] / c[3];
                let cz = c[2] / c[3];
                let on_screen = ax.abs() < 1.5
                    && ay.abs() < 1.5
                    && bx.abs() < 1.5
                    && by.abs() < 1.5
                    && cx.abs() < 1.5
                    && cy.abs() < 1.5;
                let in_depth = az > 0.0 && az < 1.0 && bz > 0.0 && bz < 1.0 && cz > 0.0 && cz < 1.0;

                if on_screen && in_depth {
                    visible += 1;
                    let winding = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);

                    if winding > 0.0 {
                        front += 1;
                    }
                }
            }

            idx += crate::world::STRIDE;
        }

        assert!(visible > 0);
        assert!(front > 0);
    }*/

    fn project(view_proj: &[f32; 16], position: [f32; 3]) -> [f32; 4] {
        let p = [position[0], position[1], position[2], 1.0];
        let mut clip = [0.0; 4];
        let mut row = 0;

        while row < 4 {
            let mut sum = 0.0;
            let mut col = 0;

            while col < 4 {
                sum += view_proj[col * 4 + row] * p[col];
                col += 1;
            }

            clip[row] = sum;
            row += 1;
        }

        clip
    }
}
