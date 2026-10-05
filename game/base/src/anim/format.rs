use crate::anim::pose::{ClipEvent, ClipSet, Sequence, Track};

pub const MESH_MAGIC: &[u8; 4] = b"EMDL";
pub const CLIP_MAGIC: &[u8; 4] = b"EANM";
pub const VERSION: u32 = 1;
pub const MESH_VERSION: u32 = 2;
pub const SURF_HITBOX: u32 = 1 << 0;
pub const SURF_SOLID: u32 = 1 << 1;
pub const SURF_TRIGGER: u32 = 1 << 2;
pub const MASK_ALL: u32 = u32::MAX;
pub const SURF_BRUSH: u32 = 1 << 3;
pub const SURF_VOXEL: u32 = 1 << 4;
pub const SURF_WORLD: u32 = SURF_BRUSH | SURF_VOXEL;
pub const SURF_PHYSICS: u32 = 1 << 5;
pub const MASK_SHOT: u32 = SURF_HITBOX | SURF_SOLID | SURF_WORLD | SURF_PHYSICS;
pub const MAX_BONES: usize = 128;
pub const MAX_NAME: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hitbox {
    pub half: [f32; 3],
    pub center: [f32; 3],
    pub rot: [f32; 4],
    pub group: u8,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Capsule {
    pub radius: f32,
    pub half_len: f32,
    pub center: [f32; 3],
    pub rot: [f32; 4],
    pub group: u8,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoneVolume {
    Capsule(Capsule),
    Hitbox(Hitbox),
}

impl BoneVolume {
    pub fn group(&self) -> u8 {
        match self {
            BoneVolume::Capsule(capsule) => capsule.group,
            BoneVolume::Hitbox(hitbox) => hitbox.group,
        }
    }

    pub fn flags(&self) -> u32 {
        match self {
            BoneVolume::Capsule(capsule) => capsule.flags,
            BoneVolume::Hitbox(hitbox) => hitbox.flags,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Bone {
    pub name: String,
    pub parent: i16,
    pub inverse_bind: [f32; 16],
    pub local_pos: [f32; 3],
    pub local_rot: [f32; 4],
    pub volume: Option<BoneVolume>,
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub bones: Vec<Bone>,
    pub vertices: Vec<f32>,
    pub indices: Vec<u32>,
    pub albedo_w: u32,
    pub albedo_h: u32,
    pub albedo: Vec<u8>,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(count)
            .ok_or_else(|| "truncated".to_string())?;

        if end > self.bytes.len() {
            return Err("truncated".to_string());
        }

        let out = &self.bytes[self.at..end];
        self.at = end;

        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let bytes = self.take(2)?;

        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.take(4)?;

        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i16(&mut self) -> Result<i16, String> {
        Ok(self.u16()? as i16)
    }

    fn f32(&mut self) -> Result<f32, String> {
        let bytes = self.take(4)?;

        Ok(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn volume(&mut self) -> Result<Option<BoneVolume>, String> {
        let tag = self.u8()?;

        if tag == 0 {
            return Ok(None);
        }

        let (half, radius, half_len) = match tag {
            1 => ([self.f32()?, self.f32()?, self.f32()?], 0.0, 0.0),
            2 => ([0.0; 3], self.f32()?, self.f32()?),
            _ => return Err("bone volume".to_string()),
        };
        let center = [self.f32()?, self.f32()?, self.f32()?];
        let rot = [self.f32()?, self.f32()?, self.f32()?, self.f32()?];
        let group = self.u8()?;
        let flags = self.u32()?;

        if tag == 1 {
            return Ok(Some(BoneVolume::Hitbox(Hitbox {
                half,
                center,
                rot,
                group,
                flags,
            })));
        }

        Ok(Some(BoneVolume::Capsule(Capsule {
            radius,
            half_len,
            center,
            rot,
            group,
            flags,
        })))
    }

    fn name(&mut self) -> Result<String, String> {
        let len = self.u16()? as usize;

        if len > MAX_NAME {
            return Err("name too long".to_string());
        }

        let bytes = self.take(len)?;

        String::from_utf8(bytes.to_vec()).map_err(|_| "name".to_string())
    }
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_i16(out: &mut Vec<u8>, value: i16) {
    push_u16(out, value as u16);
}

fn push_f32(out: &mut Vec<u8>, value: f32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_volume(out: &mut Vec<u8>, volume: &Option<BoneVolume>) {
    let (center, rot, group, flags) = match volume {
        None => {
            out.push(0);
            return;
        }
        Some(BoneVolume::Hitbox(hitbox)) => {
            out.push(1);

            for value in hitbox.half {
                push_f32(out, value);
            }

            (hitbox.center, hitbox.rot, hitbox.group, hitbox.flags)
        }
        Some(BoneVolume::Capsule(capsule)) => {
            out.push(2);
            push_f32(out, capsule.radius);
            push_f32(out, capsule.half_len);

            (capsule.center, capsule.rot, capsule.group, capsule.flags)
        }
    };

    for value in center {
        push_f32(out, value);
    }

    for value in rot {
        push_f32(out, value);
    }

    out.push(group);
    push_u32(out, flags);
}

fn push_name(out: &mut Vec<u8>, name: &str) -> Result<(), String> {
    if name.len() > MAX_NAME {
        return Err("name too long".to_string());
    }

    push_u16(out, name.len() as u16);
    out.extend_from_slice(name.as_bytes());

    Ok(())
}

pub fn write_mesh(mesh: &Mesh) -> Result<Vec<u8>, String> {
    if mesh.bones.len() > MAX_BONES {
        return Err("too many bones".to_string());
    }

    if mesh.vertices.len() % 16 != 0 {
        return Err("vertex stride".to_string());
    }

    let pixels = (mesh.albedo_w as usize)
        .checked_mul(mesh.albedo_h as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| "albedo".to_string())?;

    if mesh.albedo.len() != pixels {
        return Err("albedo size".to_string());
    }

    let mut out = Vec::new();
    out.extend_from_slice(MESH_MAGIC);
    push_u32(&mut out, MESH_VERSION);
    push_u16(&mut out, mesh.bones.len() as u16);

    for bone in &mesh.bones {
        push_name(&mut out, &bone.name)?;
        push_i16(&mut out, bone.parent);

        for value in bone.inverse_bind {
            push_f32(&mut out, value);
        }

        for value in bone.local_pos {
            push_f32(&mut out, value);
        }

        for value in bone.local_rot {
            push_f32(&mut out, value);
        }

        push_volume(&mut out, &bone.volume);
    }

    let vertex_count = (mesh.vertices.len() / 16) as u32;
    push_u32(&mut out, vertex_count);

    for value in &mesh.vertices {
        push_f32(&mut out, *value);
    }

    push_u32(&mut out, mesh.indices.len() as u32);

    for index in &mesh.indices {
        push_u32(&mut out, *index);
    }

    push_u32(&mut out, mesh.albedo_w);
    push_u32(&mut out, mesh.albedo_h);
    out.extend_from_slice(&mesh.albedo);

    Ok(out)
}

pub fn read_mesh(bytes: &[u8]) -> Result<Mesh, String> {
    let mut cursor = Cursor::new(bytes);
    let magic = cursor.take(4)?;

    if magic != MESH_MAGIC {
        return Err("mesh magic".to_string());
    }

    let version = cursor.u32()?;

    if version != VERSION && version != MESH_VERSION {
        return Err("mesh version".to_string());
    }

    let bone_count = cursor.u16()? as usize;

    if bone_count > MAX_BONES {
        return Err("too many bones".to_string());
    }

    let mut bones = Vec::with_capacity(bone_count);
    let mut idx = 0;

    while idx < bone_count {
        let name = cursor.name()?;
        let parent = cursor.i16()?;

        if parent >= idx as i16 {
            return Err("bone parent".to_string());
        }

        let mut inverse_bind = [0.0; 16];
        let mut local_pos = [0.0; 3];
        let mut local_rot = [0.0; 4];
        let mut value_idx = 0;

        while value_idx < 16 {
            inverse_bind[value_idx] = cursor.f32()?;
            value_idx += 1;
        }

        value_idx = 0;

        while value_idx < 3 {
            local_pos[value_idx] = cursor.f32()?;
            value_idx += 1;
        }

        value_idx = 0;

        while value_idx < 4 {
            local_rot[value_idx] = cursor.f32()?;
            value_idx += 1;
        }

        let volume = if version >= MESH_VERSION {
            cursor.volume()?
        } else {
            None
        };

        bones.push(Bone {
            name,
            parent,
            inverse_bind,
            local_pos,
            local_rot,
            volume,
        });
        idx += 1;
    }

    let vertex_count = cursor.u32()? as usize;

    if vertex_count > 1_000_000 {
        return Err("too many vertices".to_string());
    }

    let mut vertices = Vec::with_capacity(vertex_count * 16);
    idx = 0;

    while idx < vertex_count * 16 {
        vertices.push(cursor.f32()?);
        idx += 1;
    }

    let index_count = cursor.u32()? as usize;

    if index_count > 3_000_000 {
        return Err("too many indices".to_string());
    }

    let mut indices = Vec::with_capacity(index_count);
    idx = 0;

    while idx < index_count {
        let index = cursor.u32()?;

        if index as usize >= vertex_count {
            return Err("index".to_string());
        }

        indices.push(index);
        idx += 1;
    }

    let albedo_w = cursor.u32()?;
    let albedo_h = cursor.u32()?;
    let pixels = (albedo_w as usize)
        .checked_mul(albedo_h as usize)
        .and_then(|count| count.checked_mul(4))
        .ok_or_else(|| "albedo".to_string())?;

    if pixels > 16 * 1024 * 1024 {
        return Err("albedo".to_string());
    }

    let albedo = cursor.take(pixels)?.to_vec();

    Ok(Mesh {
        bones,
        vertices,
        indices,
        albedo_w,
        albedo_h,
        albedo,
    })
}

pub fn write_clips(clips: &ClipSet) -> Result<Vec<u8>, String> {
    if clips.bones.len() > MAX_BONES {
        return Err("too many bones".to_string());
    }

    let mut out = Vec::new();
    out.extend_from_slice(CLIP_MAGIC);
    push_u32(&mut out, VERSION);
    push_u16(&mut out, clips.bones.len() as u16);

    for name in &clips.bones {
        push_name(&mut out, name)?;
    }

    push_u16(&mut out, clips.sequences.len() as u16);

    for sequence in &clips.sequences {
        if sequence.tracks.len() != clips.bones.len() {
            return Err("track count".to_string());
        }

        push_name(&mut out, &sequence.name)?;
        push_u16(&mut out, sequence.flags);
        push_f32(&mut out, sequence.duration);
        push_u16(&mut out, sequence.events.len() as u16);

        for event in &sequence.events {
            push_f32(&mut out, event.time);
            push_name(&mut out, &event.name)?;
        }

        for track in &sequence.tracks {
            if track.pos_times.len() != track.pos.len() || track.rot_times.len() != track.rot.len()
            {
                return Err("key count".to_string());
            }

            push_u16(&mut out, track.pos.len() as u16);

            for key_idx in 0..track.pos.len() {
                push_f32(&mut out, track.pos_times[key_idx]);
                push_f32(&mut out, track.pos[key_idx][0]);
                push_f32(&mut out, track.pos[key_idx][1]);
                push_f32(&mut out, track.pos[key_idx][2]);
            }

            push_u16(&mut out, track.rot.len() as u16);

            for key_idx in 0..track.rot.len() {
                push_f32(&mut out, track.rot_times[key_idx]);
                push_f32(&mut out, track.rot[key_idx][0]);
                push_f32(&mut out, track.rot[key_idx][1]);
                push_f32(&mut out, track.rot[key_idx][2]);
                push_f32(&mut out, track.rot[key_idx][3]);
            }
        }
    }

    Ok(out)
}

pub fn read_clips(bytes: &[u8]) -> Result<ClipSet, String> {
    let mut cursor = Cursor::new(bytes);
    let magic = cursor.take(4)?;

    if magic != CLIP_MAGIC {
        return Err("clip magic".to_string());
    }

    if cursor.u32()? != VERSION {
        return Err("clip version".to_string());
    }

    let bone_count = cursor.u16()? as usize;

    if bone_count > MAX_BONES {
        return Err("too many bones".to_string());
    }

    let mut bones = Vec::with_capacity(bone_count);
    let mut idx = 0;

    while idx < bone_count {
        bones.push(cursor.name()?);
        idx += 1;
    }

    let sequence_count = cursor.u16()? as usize;

    if sequence_count > 4096 {
        return Err("too many sequences".to_string());
    }

    let mut sequences = Vec::with_capacity(sequence_count);
    idx = 0;

    while idx < sequence_count {
        let name = cursor.name()?;
        let flags = cursor.u16()?;
        let duration = cursor.f32()?;
        let event_count = cursor.u16()? as usize;

        if event_count > 256 {
            return Err("too many events".to_string());
        }

        let mut events = Vec::with_capacity(event_count);
        let mut event_idx = 0;

        while event_idx < event_count {
            let time = cursor.f32()?;
            let name = cursor.name()?;
            events.push(ClipEvent { time, name });
            event_idx += 1;
        }

        let mut tracks = Vec::with_capacity(bone_count);
        let mut bone_idx = 0;

        while bone_idx < bone_count {
            let pos_count = cursor.u16()? as usize;

            if pos_count > 8192 {
                return Err("too many keys".to_string());
            }

            let mut pos_times = Vec::with_capacity(pos_count);
            let mut pos = Vec::with_capacity(pos_count);
            let mut key_idx = 0;

            while key_idx < pos_count {
                pos_times.push(cursor.f32()?);
                pos.push([cursor.f32()?, cursor.f32()?, cursor.f32()?]);
                key_idx += 1;
            }

            let rot_count = cursor.u16()? as usize;

            if rot_count > 8192 {
                return Err("too many keys".to_string());
            }

            let mut rot_times = Vec::with_capacity(rot_count);
            let mut rot = Vec::with_capacity(rot_count);
            key_idx = 0;

            while key_idx < rot_count {
                rot_times.push(cursor.f32()?);
                rot.push([cursor.f32()?, cursor.f32()?, cursor.f32()?, cursor.f32()?]);
                key_idx += 1;
            }

            tracks.push(Track {
                pos_times,
                pos,
                rot_times,
                rot,
            });
            bone_idx += 1;
        }

        sequences.push(Sequence {
            name,
            flags,
            duration,
            events,
            tracks,
        });
        idx += 1;
    }

    Ok(ClipSet { bones, sequences })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bone(name: &str, parent: i16, volume: Option<BoneVolume>) -> Bone {
        Bone {
            name: name.to_string(),
            parent,
            inverse_bind: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            local_pos: [0.0, 1.0, 2.0],
            local_rot: [0.0, 0.0, 0.0, 1.0],
            volume,
        }
    }

    fn mesh(bones: Vec<Bone>) -> Mesh {
        Mesh {
            bones,
            vertices: Vec::new(),
            indices: Vec::new(),
            albedo_w: 1,
            albedo_h: 1,
            albedo: vec![255; 4],
        }
    }

    #[test]
    fn mesh_round_trips_bone_volumes() {
        let hitbox = BoneVolume::Hitbox(Hitbox {
            half: [1.0, 2.0, 3.0],
            center: [0.5, 0.25, -0.5],
            rot: [0.0, 0.0, 0.0, 1.0],
            group: 7,
            flags: SURF_HITBOX | SURF_SOLID,
        });
        let capsule = BoneVolume::Capsule(Capsule {
            radius: 4.0,
            half_len: 2.5,
            center: [1.0, 2.0, 3.0],
            rot: [0.0, 0.70710677, 0.0, 0.70710677],
            group: 1,
            flags: SURF_TRIGGER,
        });
        let source = mesh(vec![
            bone("root", -1, None),
            bone("spine", 0, Some(hitbox)),
            bone("head", 1, Some(capsule)),
        ]);
        let bytes = write_mesh(&source).expect("write");
        let read = read_mesh(&bytes).expect("read");

        assert_eq!(read.bones.len(), 3);
        assert_eq!(read.bones[0].volume, None);
        assert_eq!(read.bones[1].volume, Some(hitbox));
        assert_eq!(read.bones[2].volume, Some(capsule));
        assert_eq!(read.bones[2].name, "head");
        assert_eq!(read.bones[2].parent, 1);
    }

    #[test]
    fn mesh_reads_v1_without_volumes() {
        let source = mesh(vec![bone("root", -1, None)]);
        let mut bytes = write_mesh(&source).expect("write");

        // v1 had no per-bone volume byte; rewrite as v1 by dropping it and fixing the version.
        bytes[4..8].copy_from_slice(&VERSION.to_le_bytes());

        let name_len = 2 + "root".len();
        let volume_at = 4 + 4 + 2 + name_len + 2 + (16 + 3 + 4) * 4;

        assert_eq!(bytes[volume_at], 0);
        bytes.remove(volume_at);

        let read = read_mesh(&bytes).expect("read v1");

        assert_eq!(read.bones.len(), 1);
        assert_eq!(read.bones[0].volume, None);
    }

    #[test]
    fn mesh_rejects_unknown_volume_tag() {
        let source = mesh(vec![bone("root", -1, None)]);
        let mut bytes = write_mesh(&source).expect("write");
        let name_len = 2 + "root".len();
        let volume_at = 4 + 4 + 2 + name_len + 2 + (16 + 3 + 4) * 4;

        bytes[volume_at] = 9;

        assert!(read_mesh(&bytes).is_err());
    }
}

#[cfg(test)]
mod flag_tests {
    use super::*;

    #[test]
    fn surf_flags_are_distinct_and_world_combines_both() {
        let flags = [SURF_HITBOX, SURF_SOLID, SURF_TRIGGER, SURF_BRUSH, SURF_VOXEL, SURF_PHYSICS];

        for (idx, flag) in flags.iter().enumerate() {
            assert_eq!(flag.count_ones(), 1);

            for other in &flags[idx + 1..] {
                assert_eq!(flag & other, 0);
            }
        }

        assert_eq!(SURF_WORLD, SURF_BRUSH | SURF_VOXEL);
        assert_eq!(MASK_SHOT & SURF_WORLD, SURF_WORLD);
    }
}
