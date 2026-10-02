use crate::action::*;
use crate::collision::CollisionWorld;
use crate::mario::MarioState;
use crate::math::{atan2s, coss, sins};

#[inline]
pub fn mario_set_forward_vel(m: &mut MarioState, speed: f32) {
    m.forward_vel = speed;
    m.slide_vel_x = sins(m.face_angle[1]) * speed;
    m.slide_vel_z = coss(m.face_angle[1]) * speed;
    m.vel[0] = m.slide_vel_x;
    m.vel[2] = m.slide_vel_z;
}

pub fn stop_and_set_height_to_floor(m: &mut MarioState) {
    mario_set_forward_vel(m, 0.0);
    m.vel[1] = 0.0;
    m.pos[1] = m.floor_height;
}

pub fn stationary_ground_step(m: &mut MarioState) -> u32 {
    mario_set_forward_vel(m, 0.0);
    m.pos[1] = m.floor_height;
    GROUND_STEP_NONE
}

pub fn perform_ground_step(m: &mut MarioState, world: &CollisionWorld) -> u32 {
    let Some(floor_id)=m.floor else { return GROUND_STEP_HIT_WALL; };
    let Some(floor)=world.surface(floor_id) else { return GROUND_STEP_HIT_WALL; };
    let floor_normal_y=floor.normal.y;
    let mut result=GROUND_STEP_NONE;
    for _ in 0..4 {
        let next=[
            m.pos[0] + floor_normal_y * (m.vel[0] / 4.0),
            m.pos[1],
            m.pos[2] + floor_normal_y * (m.vel[2] / 4.0),
        ];
        result=perform_ground_quarter_step(m,world,next);
        if result==GROUND_STEP_LEFT_GROUND || result==GROUND_STEP_HIT_WALL_STOP_QSTEPS { break; }
    }
    if result==GROUND_STEP_HIT_WALL_CONTINUE_QSTEPS { GROUND_STEP_HIT_WALL } else { result }
}

fn perform_ground_quarter_step(m:&mut MarioState, world:&CollisionWorld, mut next:[f32;3]) -> u32 {
    let _lower=world.resolve_walls(&mut next,30.0,24.0);
    let upper=world.resolve_walls(&mut next,60.0,50.0);
    let Some(floor)=world.find_floor(next[0],next[1],next[2]) else {
        return GROUND_STEP_HIT_WALL_STOP_QSTEPS;
    };
    let ceil=world.find_ceil(next[0],floor.height,next[2]);
    let ceil_height=ceil.map_or(20000.0,|h|h.height);
    m.wall=upper;
    if next[1] > floor.height + 100.0 {
        if next[1] + 160.0 >= ceil_height { return GROUND_STEP_HIT_WALL_STOP_QSTEPS; }
        m.pos=next; m.floor=Some(floor.surface); m.floor_height=floor.height;
        return GROUND_STEP_LEFT_GROUND;
    }
    if floor.height + 160.0 >= ceil_height { return GROUND_STEP_HIT_WALL_STOP_QSTEPS; }
    m.pos=[next[0],floor.height,next[2]];
    m.floor=Some(floor.surface); m.floor_height=floor.height;
    if let Some(wall_id)=upper {
        if let Some(wall)=world.surface(wall_id) {
            let wall_dyaw=atan2s(wall.normal.z,wall.normal.x).wrapping_sub(m.face_angle[1]);
            if (0x2AAA..=0x5555).contains(&(wall_dyaw as u16))
                || ((-0x5555i16)..=(-0x2AAAi16)).contains(&wall_dyaw) {
                return GROUND_STEP_NONE;
            }
        }
        return GROUND_STEP_HIT_WALL_CONTINUE_QSTEPS;
    }
    GROUND_STEP_NONE
}

pub fn perform_air_step(m:&mut MarioState, world:&CollisionWorld, step_arg:u32) -> u32 {
    let mut result=AIR_STEP_NONE;
    let intended=[
        m.pos[0] + m.vel[0] / 4.0,
        m.pos[1] + m.vel[1] / 4.0,
        m.pos[2] + m.vel[2] / 4.0,
    ];
    for i in 0..4 {
        let next=[
            m.pos[0] + (intended[0]-m.pos[0]) / (4-i) as f32,
            m.pos[1] + (intended[1]-m.pos[1]) / (4-i) as f32,
            m.pos[2] + (intended[2]-m.pos[2]) / (4-i) as f32,
        ];
        result=perform_air_quarter_step(m,world,next,step_arg);
        if result!=AIR_STEP_NONE { break; }
    }
    result
}

fn perform_air_quarter_step(m:&mut MarioState, world:&CollisionWorld, intended:[f32;3], step_arg:u32)->u32 {
    let mut next=intended;
    let upper=world.resolve_walls(&mut next,150.0,50.0);
    let lower=world.resolve_walls(&mut next,30.0,50.0);
    let floor=world.find_floor(next[0],next[1],next[2]);
    let floor_height=floor.map_or(-11000.0,|h|h.height);
    let ceil=world.find_ceil(next[0],floor_height,next[2]);
    let ceil_height=ceil.map_or(20000.0,|h|h.height);
    m.wall=None;

    let Some(floor_hit)=floor else {
        if next[1] <= m.floor_height { m.pos[1]=m.floor_height; return AIR_STEP_LANDED; }
        m.pos[1]=next[1]; return AIR_STEP_HIT_WALL;
    };
    if next[1] <= floor_height {
        if ceil_height-floor_height > 160.0 {
            m.pos[0]=next[0]; m.pos[2]=next[2];
            m.floor=Some(floor_hit.surface); m.floor_height=floor_height;
        }
        m.pos[1]=floor_height;
        return AIR_STEP_LANDED;
    }
    if next[1]+160.0 > ceil_height {
        if m.vel[1] >= 0.0 {
            m.vel[1]=0.0;
            if step_arg & AIR_STEP_CHECK_HANG != 0 { m.ceil=ceil.map(|h|h.surface); }
            return AIR_STEP_NONE;
        }
        if next[1] <= m.floor_height { m.pos[1]=m.floor_height; return AIR_STEP_LANDED; }
        m.pos[1]=next[1]; return AIR_STEP_HIT_WALL;
    }
    if let Some(wall)=upper.or(lower) {
        m.wall=Some(wall);
        m.pos=next;
        return AIR_STEP_HIT_WALL;
    }
    m.pos=next;
    m.floor=Some(floor_hit.surface);
    m.floor_height=floor_height;
    m.ceil=ceil.map(|h|h.surface);
    m.ceil_height=ceil_height;
    AIR_STEP_NONE
}
