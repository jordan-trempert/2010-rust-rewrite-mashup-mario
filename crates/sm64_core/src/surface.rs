use crate::math::Vec3f;
use crate::types::ObjectId;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfaceNormal {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl SurfaceNormal {
    pub const fn as_vec3(self) -> Vec3f { [self.x, self.y, self.z] }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    pub surface_type: i16,
    pub force: i16,
    pub flags: i8,
    pub room: i8,
    pub lower_y: i16,
    pub upper_y: i16,
    pub vertex1: [i16; 3],
    pub vertex2: [i16; 3],
    pub vertex3: [i16; 3],
    pub normal: SurfaceNormal,
    pub origin_offset: f32,
    pub object: Option<ObjectId>,
}

impl Default for Surface {
    fn default() -> Self {
        Self {
            surface_type: 0, force: 0, flags: 0, room: 0,
            lower_y: 0, upper_y: 0,
            vertex1: [0; 3], vertex2: [0; 3], vertex3: [0; 3],
            normal: SurfaceNormal::default(), origin_offset: 0.0, object: None,
        }
    }
}

impl Surface {
    pub fn from_triangle(surface_type: i16, force: i16, flags: i8, room: i8,
                         v1: [i16; 3], v2: [i16; 3], v3: [i16; 3]) -> Option<Self> {
        let a = [
            (v2[0] - v1[0]) as f32,
            (v2[1] - v1[1]) as f32,
            (v2[2] - v1[2]) as f32,
        ];
        let b = [
            (v3[0] - v2[0]) as f32,
            (v3[1] - v2[1]) as f32,
            (v3[2] - v2[2]) as f32,
        ];
        let mut n = crate::math::vec3f_cross(a, b);
        let mag = (n[0]*n[0] + n[1]*n[1] + n[2]*n[2]).sqrt();
        if mag < 0.0001 { return None; }
        n[0] /= mag; n[1] /= mag; n[2] /= mag;
        let origin_offset = -(n[0] * v1[0] as f32 + n[1] * v1[1] as f32 + n[2] * v1[2] as f32);
        Some(Self {
            surface_type, force, flags, room,
            lower_y: v1[1].min(v2[1]).min(v3[1]) - 5,
            upper_y: v1[1].max(v2[1]).max(v3[1]) + 5,
            vertex1: v1, vertex2: v2, vertex3: v3,
            normal: SurfaceNormal { x:n[0], y:n[1], z:n[2] },
            origin_offset, object: None,
        })
    }

    #[inline]
    pub fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        if self.normal.y == 0.0 { return None; }
        Some(-(self.normal.x*x + self.normal.z*z + self.origin_offset) / self.normal.y)
    }
}
