use crate::math::Vec3f;
use crate::surface::Surface;
use crate::types::SurfaceId;

#[derive(Clone, Debug, Default)]
pub struct CollisionWorld {
    pub surfaces: Vec<Surface>,
    pub water_level: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceHit {
    pub surface: SurfaceId,
    pub height: f32,
}

impl CollisionWorld {
    pub fn clear(&mut self) { self.surfaces.clear(); }

    pub fn push_surface(&mut self, surface: Surface) -> SurfaceId {
        let id = SurfaceId(self.surfaces.len() as u32);
        self.surfaces.push(surface);
        id
    }

    pub fn surface(&self, id: SurfaceId) -> Option<&Surface> {
        self.surfaces.get(id.0 as usize)
    }

    pub fn find_floor(&self, x: f32, y: f32, z: f32) -> Option<SurfaceHit> {
        let mut best: Option<SurfaceHit> = None;
        for (index, surface) in self.surfaces.iter().enumerate() {
            if surface.normal.y <= 0.01 { continue; }
            if !point_in_triangle_xz(x, z, surface) { continue; }
            let Some(height) = surface.height_at(x, z) else { continue; };
            if height > y + 78.0 { continue; }
            if best.is_none_or(|hit| height > hit.height) {
                best = Some(SurfaceHit { surface: SurfaceId(index as u32), height });
            }
        }
        best
    }

    pub fn find_ceil(&self, x: f32, y: f32, z: f32) -> Option<SurfaceHit> {
        let mut best: Option<SurfaceHit> = None;
        for (index, surface) in self.surfaces.iter().enumerate() {
            if surface.normal.y >= -0.01 { continue; }
            if !point_in_triangle_xz(x, z, surface) { continue; }
            let Some(height) = surface.height_at(x, z) else { continue; };
            if height < y { continue; }
            if best.is_none_or(|hit| height < hit.height) {
                best = Some(SurfaceHit { surface: SurfaceId(index as u32), height });
            }
        }
        best
    }

    pub fn resolve_walls(&self, pos: &mut Vec3f, offset_y: f32, radius: f32) -> Option<SurfaceId> {
        let mut last = None;
        // SM64 makes multiple wall passes because resolving one wall can push
        // Mario into another. Keep the same style of iterative resolution.
        for _ in 0..4 {
            let mut moved = false;
            for (index, surface) in self.surfaces.iter().enumerate() {
                if surface.normal.y.abs() > 0.1 { continue; }
                let test_y = pos[1] + offset_y;
                if test_y < surface.lower_y as f32 || test_y > surface.upper_y as f32 { continue; }
                let dist = surface.normal.x * pos[0]
                    + surface.normal.y * test_y
                    + surface.normal.z * pos[2]
                    + surface.origin_offset;
                if dist < -radius || dist > radius { continue; }
                let projected = [
                    pos[0] - surface.normal.x * dist,
                    test_y - surface.normal.y * dist,
                    pos[2] - surface.normal.z * dist,
                ];
                if !point_in_triangle_3d(projected, surface) { continue; }
                let push = radius - dist;
                pos[0] += surface.normal.x * push;
                pos[2] += surface.normal.z * push;
                last = Some(SurfaceId(index as u32));
                moved = true;
            }
            if !moved { break; }
        }
        last
    }

    #[inline]
    pub fn water_level(&self, _x: f32, _z: f32) -> f32 {
        self.water_level.unwrap_or(-11000.0)
    }
}

fn edge(a: [f32;2], b: [f32;2], p: [f32;2]) -> f32 {
    (p[0]-a[0])*(b[1]-a[1]) - (p[1]-a[1])*(b[0]-a[0])
}

fn point_in_triangle_xz(x: f32, z: f32, s: &Surface) -> bool {
    let p=[x,z];
    let a=[s.vertex1[0] as f32,s.vertex1[2] as f32];
    let b=[s.vertex2[0] as f32,s.vertex2[2] as f32];
    let c=[s.vertex3[0] as f32,s.vertex3[2] as f32];
    let e1=edge(a,b,p); let e2=edge(b,c,p); let e3=edge(c,a,p);
    (e1 >= 0.0 && e2 >= 0.0 && e3 >= 0.0) || (e1 <= 0.0 && e2 <= 0.0 && e3 <= 0.0)
}

fn point_in_triangle_3d(p: Vec3f, s: &Surface) -> bool {
    let n=s.normal.as_vec3();
    let ax=n[0].abs(); let ay=n[1].abs(); let az=n[2].abs();
    let project=|v:[i16;3]| -> [f32;2] {
        if ax >= ay && ax >= az { [v[1] as f32,v[2] as f32] }
        else if ay >= az { [v[0] as f32,v[2] as f32] }
        else { [v[0] as f32,v[1] as f32] }
    };
    let pp=if ax >= ay && ax >= az {[p[1],p[2]]} else if ay >= az {[p[0],p[2]]} else {[p[0],p[1]]};
    let a=project(s.vertex1); let b=project(s.vertex2); let c=project(s.vertex3);
    let e1=edge(a,b,pp); let e2=edge(b,c,pp); let e3=edge(c,a,pp);
    (e1 >= 0.0 && e2 >= 0.0 && e3 >= 0.0) || (e1 <= 0.0 && e2 <= 0.0 && e3 <= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn floor_query_returns_triangle_height() {
        let mut world=CollisionWorld::default();
        world.push_surface(Surface::from_triangle(0,0,0,0,[-100,0,-100],[100,0,-100],[0,0,100]).unwrap());
        let hit=world.find_floor(0.0,50.0,0.0).unwrap();
        assert!(hit.height.abs() < 0.001);
    }
}
