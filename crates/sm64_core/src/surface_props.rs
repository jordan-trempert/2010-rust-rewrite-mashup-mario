use crate::action::ACT_CRAWLING;
use crate::collision::CollisionWorld;
use crate::mario::MarioState;
use crate::surface_types::*;

pub fn mario_get_floor_class(m:&MarioState, world:&CollisionWorld, terrain_type:i16)->i32 {
    let mut class=if (terrain_type as i32 & TERRAIN_MASK)==TERRAIN_SLIDE {
        SURFACE_CLASS_VERY_SLIPPERY
    } else {
        SURFACE_CLASS_DEFAULT
    };
    if let Some(floor)=m.floor.and_then(|id|world.surface(id)) {
        class=match floor.surface_type as i32 {
            SURFACE_NOT_SLIPPERY | SURFACE_HARD_NOT_SLIPPERY | SURFACE_SWITCH => SURFACE_CLASS_NOT_SLIPPERY,
            SURFACE_SLIPPERY | SURFACE_NOISE_SLIPPERY | SURFACE_HARD_SLIPPERY
            | SURFACE_NO_CAM_COL_SLIPPERY => SURFACE_CLASS_SLIPPERY,
            SURFACE_VERY_SLIPPERY | SURFACE_ICE | SURFACE_HARD_VERY_SLIPPERY
            | SURFACE_NOISE_VERY_SLIPPERY_73 | SURFACE_NOISE_VERY_SLIPPERY_74
            | SURFACE_NOISE_VERY_SLIPPERY | SURFACE_NO_CAM_COL_VERY_SLIPPERY => SURFACE_CLASS_VERY_SLIPPERY,
            _ => class,
        };
        if m.action==ACT_CRAWLING && floor.normal.y>0.5 && class==SURFACE_CLASS_DEFAULT {
            class=SURFACE_CLASS_NOT_SLIPPERY;
        }
    }
    class
}

#[inline]
pub fn mario_facing_downhill(m:&MarioState, turn_yaw:bool)->bool {
    let mut yaw=m.face_angle[1];
    if turn_yaw && m.forward_vel<0.0 { yaw=yaw.wrapping_add(i16::MIN); }
    let d=m.floor_angle.wrapping_sub(yaw);
    d > -0x4000 && d < 0x4000
}

pub fn mario_floor_is_slippery(m:&MarioState, world:&CollisionWorld, terrain_type:i16)->bool {
    let Some(floor)=m.floor.and_then(|id|world.surface(id)) else {return false;};
    if (terrain_type as i32 & TERRAIN_MASK)==TERRAIN_SLIDE && floor.normal.y<0.9998477 {
        return true;
    }
    let norm_y=match mario_get_floor_class(m,world,terrain_type) {
        SURFACE_CLASS_VERY_SLIPPERY => 0.9848077,
        SURFACE_CLASS_SLIPPERY => 0.9396926,
        SURFACE_CLASS_NOT_SLIPPERY => 0.0,
        _ => 0.7880108,
    };
    floor.normal.y<=norm_y
}

pub fn mario_floor_is_slope(m:&MarioState, world:&CollisionWorld, terrain_type:i16)->bool {
    let Some(floor)=m.floor.and_then(|id|world.surface(id)) else {return false;};
    if (terrain_type as i32 & TERRAIN_MASK)==TERRAIN_SLIDE && floor.normal.y<0.9998477 {
        return true;
    }
    let norm_y=match mario_get_floor_class(m,world,terrain_type) {
        SURFACE_CLASS_VERY_SLIPPERY => 0.9961947,
        SURFACE_CLASS_SLIPPERY => 0.9848077,
        SURFACE_CLASS_NOT_SLIPPERY => 0.9396926,
        _ => 0.9659258,
    };
    floor.normal.y<=norm_y
}

pub fn mario_floor_is_steep(m:&MarioState, world:&CollisionWorld, terrain_type:i16)->bool {
    if mario_facing_downhill(m,false) {return false;}
    let Some(floor)=m.floor.and_then(|id|world.surface(id)) else {return false;};
    let norm_y=match mario_get_floor_class(m,world,terrain_type) {
        SURFACE_CLASS_VERY_SLIPPERY => 0.9659258,
        SURFACE_CLASS_SLIPPERY => 0.9396926,
        SURFACE_CLASS_NOT_SLIPPERY => 0.8660254,
        _ => 0.8660254,
    };
    floor.normal.y<=norm_y
}
