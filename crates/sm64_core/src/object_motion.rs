use crate::{CollisionWorld, Sm64Object, math::{coss, sins}};

pub const OBJ_MOVE_LANDED:u32=1<<0;
pub const OBJ_MOVE_ON_GROUND:u32=1<<1;
pub const OBJ_MOVE_LEFT_GROUND:u32=1<<2;
pub const OBJ_MOVE_ENTERED_WATER:u32=1<<3;
pub const OBJ_MOVE_AT_WATER_SURFACE:u32=1<<4;
pub const OBJ_MOVE_UNDERWATER_OFF_GROUND:u32=1<<5;
pub const OBJ_MOVE_UNDERWATER_ON_GROUND:u32=1<<6;
pub const OBJ_MOVE_IN_AIR:u32=1<<7;
pub const OBJ_MOVE_OUT_SCOPE:u32=1<<8;
pub const OBJ_MOVE_HIT_WALL:u32=1<<9;
pub const OBJ_MOVE_HIT_EDGE:u32=1<<10;
pub const OBJ_MOVE_ABOVE_LAVA:u32=1<<11;
pub const OBJ_MOVE_LEAVING_WATER:u32=1<<12;
pub const OBJ_MOVE_BOUNCE:u32=1<<13;
pub const OBJ_MOVE_ABOVE_DEATH_BARRIER:u32=1<<14;

pub const OBJ_MOVE_MASK_ON_GROUND:u32=OBJ_MOVE_LANDED|OBJ_MOVE_ON_GROUND;
pub const OBJ_MOVE_MASK_IN_WATER:u32=OBJ_MOVE_ENTERED_WATER
    |OBJ_MOVE_AT_WATER_SURFACE
    |OBJ_MOVE_UNDERWATER_OFF_GROUND
    |OBJ_MOVE_UNDERWATER_ON_GROUND;

/// Shared 30 Hz movement for ordinary SM64 behavior objects. Behavior ports
/// can copy gravity, friction, buoyancy, bounciness and forward velocity from
/// the decomp without converting them to per-second values.
pub fn object_step(object:&mut Sm64Object, collision:&CollisionWorld) -> u32 {
    let previous=object.move_flags;
    object.move_flags=0;

    object.vel[0]=sins(object.move_angle[1])*object.forward_vel;
    object.vel[2]=coss(object.move_angle[1])*object.forward_vel;

    let old=object.pos;
    object.pos[0]+=object.vel[0];
    object.pos[2]+=object.vel[2];

    let before_walls=object.pos;
    collision.resolve_walls(&mut object.pos,50.0,50.0);
    if (object.pos[0]-before_walls[0]).abs()>0.01
        || (object.pos[2]-before_walls[2]).abs()>0.01
    {
        object.move_flags|=OBJ_MOVE_HIT_WALL;
    }

    let water=collision.water_level(object.pos[0],object.pos[2]);
    let was_in_water=previous&OBJ_MOVE_MASK_IN_WATER!=0;
    let underwater=object.pos[1] < water;

    object.pos[1]+=object.vel[1];
    if underwater {
        // The common underwater object path damps vertical velocity and adds
        // buoyancy rather than normal gravity.
        object.vel[1]*=0.8;
        object.vel[1]+=object.buoyancy;
        object.forward_vel*=0.8;
    } else {
        object.vel[1]+=object.gravity;
    }

    let floor=collision.find_floor(object.pos[0],object.pos[1]+100.0,object.pos[2]);
    if let Some(hit)=floor {
        object.floor=Some(hit.surface);
        object.floor_height=hit.height;
        object.floor_type=collision.surface(hit.surface).map_or(0,|surface|surface.surface_type);

        if object.pos[1] <= hit.height {
            let was_airborne=previous&OBJ_MOVE_MASK_ON_GROUND==0;
            object.pos[1]=hit.height;
            if underwater {
                object.vel[1]=0.0;
                object.move_flags|=OBJ_MOVE_UNDERWATER_ON_GROUND;
            } else if object.vel[1] < 0.0 && object.bounciness.abs()>0.01 {
                object.vel[1]=-object.vel[1]*object.bounciness;
                object.move_flags|=OBJ_MOVE_BOUNCE;
                if object.vel[1] > 5.0 {
                    object.move_flags|=OBJ_MOVE_IN_AIR;
                } else {
                    object.vel[1]=0.0;
                    object.move_flags|=if was_airborne{OBJ_MOVE_LANDED}else{OBJ_MOVE_ON_GROUND};
                }
            } else {
                object.vel[1]=0.0;
                object.move_flags|=if was_airborne{OBJ_MOVE_LANDED}else{OBJ_MOVE_ON_GROUND};
            }
            object.forward_vel*=object.friction;
        } else if underwater {
            object.move_flags|=OBJ_MOVE_UNDERWATER_OFF_GROUND;
        } else {
            object.move_flags|=OBJ_MOVE_IN_AIR;
            if previous&OBJ_MOVE_MASK_ON_GROUND!=0 { object.move_flags|=OBJ_MOVE_LEFT_GROUND; }
        }
    } else {
        object.floor=None;
        object.floor_height=-11000.0;
        object.floor_type=0;
        object.move_flags|=if underwater{OBJ_MOVE_UNDERWATER_OFF_GROUND}else{OBJ_MOVE_IN_AIR};
    }

    if underwater {
        if !was_in_water { object.move_flags|=OBJ_MOVE_ENTERED_WATER; }
        if object.pos[1] >= water-100.0 { object.move_flags|=OBJ_MOVE_AT_WATER_SURFACE; }
    } else if was_in_water {
        object.move_flags|=OBJ_MOVE_LEAVING_WATER;
    }

    if floor.is_none() && old[1] >= -10000.0 {
        object.pos[0]=old[0];
        object.pos[2]=old[2];
        object.move_flags|=OBJ_MOVE_HIT_EDGE;
    }

    object.vel[0]=sins(object.move_angle[1])*object.forward_vel;
    object.vel[2]=coss(object.move_angle[1])*object.forward_vel;
    object.move_flags
}

#[inline]
pub fn approach_i16(current:i16,target:i16,step:i16)->i16 {
    let delta=target.wrapping_sub(current);
    if delta>step { current.wrapping_add(step) }
    else if delta < -step { current.wrapping_sub(step) }
    else { target }
}

pub fn rotate_yaw_toward(object:&mut Sm64Object,target:i16,increment:i16)->bool {
    object.move_angle[1]=approach_i16(object.move_angle[1],target,increment.abs());
    object.face_angle[1]=object.move_angle[1];
    object.move_angle[1]==target
}
