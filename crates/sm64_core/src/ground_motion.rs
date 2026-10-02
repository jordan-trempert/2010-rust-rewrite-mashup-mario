use crate::action::*;
use crate::collision::CollisionWorld;
use crate::mario::MarioState;
use crate::math::{atan2s, coss, sins};
use crate::mario_step::mario_set_forward_vel;
use crate::surface_props::{mario_floor_is_slope, mario_get_floor_class};
use crate::surface_types::*;

const MOVING_SAND_SPEEDS:[f32;4]=[12.0,8.0,4.0,0.0];

pub fn mario_update_moving_sand(m:&mut MarioState, world:&CollisionWorld)->bool {
    let Some(floor)=m.floor.and_then(|id|world.surface(id)) else{return false;};
    match floor.surface_type as i32 {
        SURFACE_DEEP_MOVING_QUICKSAND | SURFACE_SHALLOW_MOVING_QUICKSAND
        | SURFACE_MOVING_QUICKSAND | SURFACE_INSTANT_MOVING_QUICKSAND => {
            let push_angle=(floor.force as i32).wrapping_shl(8) as i16;
            let index=((floor.force as u16)>>8) as usize;
            let speed=*MOVING_SAND_SPEEDS.get(index).unwrap_or(&0.0);
            m.vel[0]+=speed*sins(push_angle);
            m.vel[2]+=speed*coss(push_angle);
            true
        }
        _=>false,
    }
}

pub fn mario_update_windy_ground(m:&mut MarioState, world:&CollisionWorld)->bool {
    let Some(floor)=m.floor.and_then(|id|world.surface(id)) else{return false;};
    if floor.surface_type as i32 != SURFACE_HORIZONTAL_WIND {return false;}
    let push_angle=(floor.force as i32).wrapping_shl(8) as i16;
    let mut push_speed;
    if m.action & ACT_FLAG_MOVING != 0 {
        let push_dyaw=m.face_angle[1].wrapping_sub(push_angle);
        push_speed=if m.forward_vel>0.0 {-m.forward_vel*0.5}else{-8.0};
        if push_dyaw>-0x4000 && push_dyaw<0x4000 {push_speed*=-1.0;}
        push_speed*=coss(push_dyaw);
    } else {
        push_speed=3.2+(m.global_timer%4) as f32;
    }
    m.vel[0]+=push_speed*sins(push_angle);
    m.vel[2]+=push_speed*coss(push_angle);
    true
}

pub fn apply_slope_accel(m:&mut MarioState, world:&CollisionWorld) {
    let Some(floor)=m.floor.and_then(|id|world.surface(id)) else{return;};
    let steepness=(floor.normal.x*floor.normal.x+floor.normal.z*floor.normal.z).sqrt();
    let floor_dyaw=m.floor_angle.wrapping_sub(m.face_angle[1]);
    if mario_floor_is_slope(m,world) {
        let class=if m.action!=ACT_SOFT_BACKWARD_GROUND_KB && m.action!=ACT_SOFT_FORWARD_GROUND_KB {
            mario_get_floor_class(m,world)
        } else {0};
        let accel=match class {
            SURFACE_CLASS_VERY_SLIPPERY=>5.3,
            SURFACE_CLASS_SLIPPERY=>2.7,
            SURFACE_CLASS_NOT_SLIPPERY=>0.0,
            _=>1.7,
        };
        if floor_dyaw>-0x4000 && floor_dyaw<0x4000 {
            m.forward_vel+=accel*steepness;
        } else {
            m.forward_vel-=accel*steepness;
        }
    }
    m.slide_yaw=m.face_angle[1];
    m.slide_vel_x=m.forward_vel*sins(m.face_angle[1]);
    m.slide_vel_z=m.forward_vel*coss(m.face_angle[1]);
    m.vel=[m.slide_vel_x,0.0,m.slide_vel_z];
    mario_update_moving_sand(m,world);
    mario_update_windy_ground(m,world);
}

pub fn update_sliding_angle(m:&mut MarioState, world:&CollisionWorld, accel:f32, loss_factor:f32) {
    let Some(floor)=m.floor.and_then(|id|world.surface(id)) else{return;};
    let slope_angle=atan2s(floor.normal.z,floor.normal.x);
    let steepness=(floor.normal.x*floor.normal.x+floor.normal.z*floor.normal.z).sqrt();
    m.slide_vel_x+=accel*steepness*sins(slope_angle);
    m.slide_vel_z+=accel*steepness*coss(slope_angle);
    m.slide_vel_x*=loss_factor;
    m.slide_vel_z*=loss_factor;
    m.slide_yaw=atan2s(m.slide_vel_z,m.slide_vel_x);

    let facing_dyaw=m.face_angle[1].wrapping_sub(m.slide_yaw);
    let mut new=facing_dyaw as i32;
    if new>0 && new<=0x4000 {
        new-=0x200; if new<0 {new=0;}
    } else if new>-0x4000 && new<0 {
        new+=0x200; if new>0 {new=0;}
    } else if new>0x4000 && new<0x8000 {
        new+=0x200; if new>0x8000 {new=0x8000;}
    } else if new>-0x8000 && new< -0x4000 {
        new-=0x200; if new< -0x8000 {new=-0x8000;}
    }
    m.face_angle[1]=m.slide_yaw.wrapping_add(new as i16);
    m.vel=[m.slide_vel_x,0.0,m.slide_vel_z];
    mario_update_moving_sand(m,world);
    mario_update_windy_ground(m,world);

    m.forward_vel=(m.slide_vel_x*m.slide_vel_x+m.slide_vel_z*m.slide_vel_z).sqrt();
    if m.forward_vel>100.0 {
        m.slide_vel_x=m.slide_vel_x*100.0/m.forward_vel;
        m.slide_vel_z=m.slide_vel_z*100.0/m.forward_vel;
    }
    if new< -0x4000 || new>0x4000 {m.forward_vel*=-1.0;}
}

pub fn update_sliding(m:&mut MarioState, world:&CollisionWorld, stop_speed:f32)->bool {
    let intended_dyaw=m.intended_yaw.wrapping_sub(m.slide_yaw);
    let mut forward=coss(intended_dyaw);
    let sideward=sins(intended_dyaw);
    if forward<0.0 && m.forward_vel>=0.0 {
        forward*=0.5+0.5*m.forward_vel/100.0;
    }
    let (accel,loss)=match mario_get_floor_class(m,world) {
        SURFACE_CLASS_VERY_SLIPPERY=>(10.0,m.intended_mag/32.0*forward*0.02+0.98),
        SURFACE_CLASS_SLIPPERY=>(8.0,m.intended_mag/32.0*forward*0.02+0.96),
        SURFACE_CLASS_NOT_SLIPPERY=>(5.0,m.intended_mag/32.0*forward*0.02+0.92),
        _=>(7.0,m.intended_mag/32.0*forward*0.02+0.92),
    };
    let old_speed=(m.slide_vel_x*m.slide_vel_x+m.slide_vel_z*m.slide_vel_z).sqrt();
    m.slide_vel_x+=m.slide_vel_z*(m.intended_mag/32.0)*sideward*0.05;
    m.slide_vel_z-=m.slide_vel_x*(m.intended_mag/32.0)*sideward*0.05;
    let new_speed=(m.slide_vel_x*m.slide_vel_x+m.slide_vel_z*m.slide_vel_z).sqrt();
    if old_speed>0.0 && new_speed>0.0 {
        m.slide_vel_x=m.slide_vel_x*old_speed/new_speed;
        m.slide_vel_z=m.slide_vel_z*old_speed/new_speed;
    }
    update_sliding_angle(m,world,accel,loss);
    if !mario_floor_is_slope(m,world) && m.forward_vel*m.forward_vel<stop_speed*stop_speed {
        mario_set_forward_vel(m,0.0);
        return true;
    }
    false
}

pub fn should_begin_sliding(m:&MarioState, world:&CollisionWorld)->bool {
    if m.input & INPUT_ABOVE_SLIDE as u16 == 0 {return false;}
    let slide_level=(m.terrain_type as i32 & TERRAIN_MASK)==TERRAIN_SLIDE;
    let moving_backward=m.forward_vel<=-1.0;
    slide_level || moving_backward || crate::surface_props::mario_facing_downhill(m,false)
}

#[inline]
pub fn analog_stick_held_back(m:&MarioState)->bool {
    let d=m.intended_yaw.wrapping_sub(m.face_angle[1]);
    d < -0x471C || d > 0x471C
}
