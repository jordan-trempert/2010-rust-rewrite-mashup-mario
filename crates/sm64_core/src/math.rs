pub type Vec3f = [f32; 3];
pub type Vec3s = [i16; 3];

pub const ANGLE_FULL_TURN: f32 = 65536.0;
pub const ANGLE_HALF_TURN: i16 = i16::MIN;

#[inline]
pub const fn vec3f(x: f32, y: f32, z: f32) -> Vec3f { [x, y, z] }

#[inline]
pub const fn vec3s(x: i16, y: i16, z: i16) -> Vec3s { [x, y, z] }

#[inline]
pub fn sins(angle: i16) -> f32 {
    crate::trig_tables::SINE_COSINE_TABLE[(angle as u16 as usize) >> 4]
}

#[inline]
pub fn coss(angle: i16) -> f32 {
    crate::trig_tables::SINE_COSINE_TABLE[((angle as u16 as usize) >> 4) + 0x400]
}

#[inline]
pub fn atan2s(mut y: f32, mut x: f32) -> i16 {
    #[inline]
    fn lookup(y: f32, x: f32) -> u16 {
        let index = if x == 0.0 { 0 } else { (y / x * 1024.0 + 0.5) as usize };
        crate::trig_tables::ARCTAN_TABLE[index.min(0x400)] as u16
    }
    let ret: u16;
    if x >= 0.0 {
        if y >= 0.0 {
            if y >= x { ret = lookup(x, y); } else { ret = 0x4000u16.wrapping_sub(lookup(y, x)); }
        } else {
            y = -y;
            if y < x { ret = 0x4000u16.wrapping_add(lookup(y, x)); } else { ret = 0x8000u16.wrapping_sub(lookup(x, y)); }
        }
    } else {
        x = -x;
        if y < 0.0 {
            y = -y;
            if y >= x { ret = 0x8000u16.wrapping_add(lookup(x, y)); } else { ret = 0xC000u16.wrapping_sub(lookup(y, x)); }
        } else if y < x {
            ret = 0xC000u16.wrapping_add(lookup(y, x));
        } else {
            ret = 0u16.wrapping_sub(lookup(x, y));
        }
    }
    ret as i16
}

#[inline]
pub fn approach_i32(mut current: i32, target: i32, inc: i32, dec: i32) -> i32 {
    if current < target {
        current = current.wrapping_add(inc);
        if current > target { current = target; }
    } else {
        current = current.wrapping_sub(dec);
        if current < target { current = target; }
    }
    current
}

#[inline]
pub fn approach_f32(mut current: f32, target: f32, inc: f32, dec: f32) -> f32 {
    if current < target {
        current += inc;
        if current > target { current = target; }
    } else {
        current -= dec;
        if current < target { current = target; }
    }
    current
}

#[inline]
pub fn vec3f_copy(src: Vec3f) -> Vec3f { src }

#[inline]
pub fn vec3f_add(a: &mut Vec3f, b: Vec3f) {
    a[0] += b[0]; a[1] += b[1]; a[2] += b[2];
}

#[inline]
pub fn vec3f_sum(a: Vec3f, b: Vec3f) -> Vec3f {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

#[inline]
pub fn vec3f_cross(a: Vec3f, b: Vec3f) -> Vec3f {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[inline]
pub fn vec3f_normalize(v: &mut Vec3f) {
    let mag = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if mag != 0.0 {
        v[0] /= mag; v[1] /= mag; v[2] /= mag;
    }
}

#[inline]
pub fn dist_and_angle(from: Vec3f, to: Vec3f) -> (f32, i16, i16) {
    let x = to[0] - from[0];
    let y = to[1] - from[1];
    let z = to[2] - from[2];
    let dist = (x * x + y * y + z * z).sqrt();
    (dist, atan2s((x * x + z * z).sqrt(), y), atan2s(z, x))
}

#[inline]
pub fn set_dist_and_angle(from: Vec3f, dist: f32, pitch: i16, yaw: i16) -> Vec3f {
    [
        from[0] + dist * coss(pitch) * sins(yaw),
        from[1] + dist * sins(pitch),
        from[2] + dist * coss(pitch) * coss(yaw),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cardinal_angles_follow_sm64_units() {
        assert!((sins(0) - 0.0).abs() < 1e-6);
        assert!((coss(0) - 1.0).abs() < 1e-6);
        assert!((sins(0x4000) - 1.0).abs() < 1e-5);
        assert!((coss(0x4000)).abs() < 1e-5);
    }
}
