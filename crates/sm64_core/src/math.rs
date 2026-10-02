pub type Vec3f = [f32; 3];
pub type Vec3s = [i16; 3];

#[inline]
pub const fn vec3f(x: f32, y: f32, z: f32) -> Vec3f {
    [x, y, z]
}

#[inline]
pub const fn vec3s(x: i16, y: i16, z: i16) -> Vec3s {
    [x, y, z]
}
