use crate::math::Vec3f;
use crate::surface::Surface;
use crate::surface_types::*;
use crate::types::SurfaceId;

pub const FLOOR_LOWER_LIMIT: f32 = -11000.0;
pub const CELL_HEIGHT_LIMIT: f32 = 20000.0;

#[derive(Clone, Debug, Default)]
pub struct CollisionWorld {
    /// Surfaces are retained in load order. The decomp's spatial lists are
    /// ordered, and many queries intentionally return the first matching surface.
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

    pub fn push_surface(&mut self, mut surface: Surface) -> SurfaceId {
        // surface_load.c marks near-X-facing walls for YZ projection.
        if surface.normal.y.abs() <= 0.01 && surface.normal.x.abs() > 0.707 {
            surface.flags |= SURFACE_FLAG_X_PROJECTION as i8;
        }
        let id=SurfaceId(self.surfaces.len() as u32);
        self.surfaces.push(surface);
        id
    }

    pub fn surface(&self,id:SurfaceId)->Option<&Surface> { self.surfaces.get(id.0 as usize) }

    pub fn find_floor(&self,x_pos:f32,y_pos:f32,z_pos:f32)->Option<SurfaceHit> {
        // Preserve the original s16 position cast (including parallel-universe wrap).
        let x=x_pos as i16 as i32;
        let y=y_pos as i16 as i32;
        let z=z_pos as i16 as i32;
        for (index,s) in self.surfaces.iter().enumerate() {
            if s.normal.y <= 0.01 {continue;}
            let x1=s.vertex1[0] as i32; let z1=s.vertex1[2] as i32;
            let x2=s.vertex2[0] as i32; let z2=s.vertex2[2] as i32;
            if (z1-z)*(x2-x1)-(x1-x)*(z2-z1)<0 {continue;}
            let x3=s.vertex3[0] as i32; let z3=s.vertex3[2] as i32;
            if (z2-z)*(x3-x2)-(x2-x)*(z3-z2)<0 {continue;}
            if (z3-z)*(x1-x3)-(x3-x)*(z1-z3)<0 {continue;}
            if s.surface_type as i32==SURFACE_CAMERA_BOUNDARY {continue;}
            let ny=s.normal.y;
            if ny==0.0 {continue;}
            let height=-(x as f32*s.normal.x+s.normal.z*z as f32+s.origin_offset)/ny;
            if y as f32-(height-78.0)<0.0 {continue;}
            return Some(SurfaceHit{surface:SurfaceId(index as u32),height});
        }
        None
    }

    pub fn find_ceil(&self,x_pos:f32,y_pos:f32,z_pos:f32)->Option<SurfaceHit> {
        let x=x_pos as i16 as i32;
        let y=y_pos as i16 as i32;
        let z=z_pos as i16 as i32;
        for (index,s) in self.surfaces.iter().enumerate() {
            if s.normal.y >= -0.01 {continue;}
            let x1=s.vertex1[0] as i32; let z1=s.vertex1[2] as i32;
            let x2=s.vertex2[0] as i32; let z2=s.vertex2[2] as i32;
            if (z1-z)*(x2-x1)-(x1-x)*(z2-z1)>0 {continue;}
            let x3=s.vertex3[0] as i32; let z3=s.vertex3[2] as i32;
            if (z2-z)*(x3-x2)-(x2-x)*(z3-z2)>0 {continue;}
            if (z3-z)*(x1-x3)-(x3-x)*(z1-z3)>0 {continue;}
            if s.surface_type as i32==SURFACE_CAMERA_BOUNDARY {continue;}
            let ny=s.normal.y;
            if ny==0.0 {continue;}
            let height=-(x as f32*s.normal.x+s.normal.z*z as f32+s.origin_offset)/ny;
            // Exact exposed-ceiling 78-unit buffer check.
            if y as f32-(height+78.0)>0.0 {continue;}
            return Some(SurfaceHit{surface:SurfaceId(index as u32),height});
        }
        None
    }

    pub fn resolve_walls(&self,pos:&mut Vec3f,offset_y:f32,radius:f32)->Option<SurfaceId> {
        let radius=radius.min(200.0);
        let x=pos[0];
        let y=pos[1]+offset_y;
        let z=pos[2];
        let mut referenced: [Option<SurfaceId>;4]=[None;4];
        let mut num_walls=0usize;

        for (index,s) in self.surfaces.iter().enumerate() {
            if s.normal.y.abs()>0.01 {continue;}
            if y<s.lower_y as f32 || y>s.upper_y as f32 {continue;}
            let offset=s.normal.x*x+s.normal.y*y+s.normal.z*z+s.origin_offset;
            if offset < -radius || offset > radius {continue;}

            let inside=if s.flags as i32 & SURFACE_FLAG_X_PROJECTION != 0 {
                wall_inside_x_projection(y,z,s)
            } else {
                wall_inside_z_projection(x,y,s)
            };
            if !inside {continue;}
            if s.surface_type as i32==SURFACE_CAMERA_BOUNDARY {continue;}

            // Deliberately use the original x/z for every wall offset. The C
            // routine accumulates pushes in data->x/z without updating locals.
            pos[0]+=s.normal.x*(radius-offset);
            pos[2]+=s.normal.z*(radius-offset);
            if num_walls<4 {
                referenced[num_walls]=Some(SurfaceId(index as u32));
                num_walls+=1;
            }
        }
        if num_walls==0 {None}else{referenced[num_walls-1]}
    }

    #[inline]
    pub fn water_level(&self,_x:f32,_z:f32)->f32 {self.water_level.unwrap_or(FLOOR_LOWER_LIMIT)}
}

fn wall_inside_x_projection(y:f32,z:f32,s:&Surface)->bool {
    let w1=-(s.vertex1[2] as f32); let w2=-(s.vertex2[2] as f32); let w3=-(s.vertex3[2] as f32);
    let y1=s.vertex1[1] as f32; let y2=s.vertex2[1] as f32; let y3=s.vertex3[1] as f32;
    let pz=-z;
    let e1=(y1-y)*(w2-w1)-(w1-pz)*(y2-y1);
    let e2=(y2-y)*(w3-w2)-(w2-pz)*(y3-y2);
    let e3=(y3-y)*(w1-w3)-(w3-pz)*(y1-y3);
    if s.normal.x>0.0 {e1<=0.0&&e2<=0.0&&e3<=0.0}else{e1>=0.0&&e2>=0.0&&e3>=0.0}
}

fn wall_inside_z_projection(x:f32,y:f32,s:&Surface)->bool {
    let w1=s.vertex1[0] as f32; let w2=s.vertex2[0] as f32; let w3=s.vertex3[0] as f32;
    let y1=s.vertex1[1] as f32; let y2=s.vertex2[1] as f32; let y3=s.vertex3[1] as f32;
    let e1=(y1-y)*(w2-w1)-(w1-x)*(y2-y1);
    let e2=(y2-y)*(w3-w2)-(w2-x)*(y3-y2);
    let e3=(y3-y)*(w1-w3)-(w3-x)*(y1-y3);
    if s.normal.z>0.0 {e1<=0.0&&e2<=0.0&&e3<=0.0}else{e1>=0.0&&e2>=0.0&&e3>=0.0}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn floor_query_returns_triangle_height() {
        let mut world=CollisionWorld::default();
        world.push_surface(Surface::from_triangle(0,0,0,0,[-100,0,-100],[100,0,-100],[0,0,100]).unwrap());
        let hit=world.find_floor(0.0,50.0,0.0).unwrap();
        assert!(hit.height.abs()<0.001);
    }
}
