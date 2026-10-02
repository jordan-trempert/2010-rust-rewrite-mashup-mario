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
    m.wall=None;
    let mut step_result=AIR_STEP_NONE;
    for _ in 0..4 {
        let intended=[
            m.pos[0] + m.vel[0] / 4.0,
            m.pos[1] + m.vel[1] / 4.0,
            m.pos[2] + m.vel[2] / 4.0,
        ];
        let quarter=perform_air_quarter_step(m,world,intended,step_arg);
        if quarter != AIR_STEP_NONE { step_result=quarter; }
        if quarter==AIR_STEP_LANDED
            || quarter==AIR_STEP_GRABBED_LEDGE
            || quarter==AIR_STEP_GRABBED_CEILING
            || quarter==AIR_STEP_HIT_LAVA_WALL {
            break;
        }
    }
    if m.vel[1] >= 0.0 { m.peak_height=m.pos[1]; }
    if m.action != ACT_FLYING { apply_gravity(m); }
    step_result
}

fn perform_air_quarter_step(m:&mut MarioState, world:&CollisionWorld, intended:[f32;3], step_arg:u32)->u32 {
    let mut next=intended;
    let upper=world.resolve_walls(&mut next,150.0,50.0);
    let lower=world.resolve_walls(&mut next,30.0,50.0);
    let floor=world.find_floor(next[0],next[1],next[2]);
    let floor_height=floor.map_or(-11000.0,|h|h.height);
    let ceil=world.find_ceil(next[0],floor_height + 80.0,next[2]);
    let ceil_height=ceil.map_or(20000.0,|h|h.height);
    m.wall=None;

    let Some(floor_hit)=floor else {
        if next[1] <= m.floor_height { m.pos[1]=m.floor_height; return AIR_STEP_LANDED; }
        m.pos[1]=next[1];
        return AIR_STEP_HIT_WALL;
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
            if step_arg & AIR_STEP_CHECK_HANG != 0 {
                if m.ceil
                    .and_then(|id|world.surface(id))
                    .is_some_and(|s|s.surface_type as i32==SURFACE_HANGABLE)
                {
                    return AIR_STEP_GRABBED_CEILING;
                }
            }
            return AIR_STEP_NONE;
        }
        if next[1] <= m.floor_height { m.pos[1]=m.floor_height; return AIR_STEP_LANDED; }
        m.pos[1]=next[1];
        return AIR_STEP_HIT_WALL;
    }

    if step_arg & AIR_STEP_CHECK_LEDGE_GRAB != 0 && upper.is_none() {
        if let Some(lower_wall)=lower {
            if check_ledge_grab(m,world,lower_wall,intended,next) {
                return AIR_STEP_GRABBED_LEDGE;
            }
            m.pos=next;
            m.floor=Some(floor_hit.surface);
            m.floor_height=floor_height;
            return AIR_STEP_NONE;
        }
    }

    m.pos=next;
    m.floor=Some(floor_hit.surface);
    m.floor_height=floor_height;

    if let Some(wall)=upper.or(lower) {
        m.wall=Some(wall);
        if world.surface(wall).is_some_and(|s| s.surface_type as i32 == SURFACE_BURNING) {
            return AIR_STEP_HIT_LAVA_WALL;
        }
        if let Some(surface)=world.surface(wall) {
            let wall_dyaw=atan2s(surface.normal.z,surface.normal.x).wrapping_sub(m.face_angle[1]);
            if wall_dyaw < -0x6000 || wall_dyaw > 0x6000 {
                m.flags |= MARIO_UNKNOWN_30;
                return AIR_STEP_HIT_WALL;
            }
        }
    }
    m.ceil=ceil.map(|h|h.surface);
    m.ceil_height=ceil_height;
    AIR_STEP_NONE
}

fn check_ledge_grab(
    m:&mut MarioState,
    world:&CollisionWorld,
    wall_id:crate::types::SurfaceId,
    intended:[f32;3],
    next:[f32;3],
)->bool {
    if m.vel[1]>0.0 {return false;}
    let displacement_x=next[0]-intended[0];
    let displacement_z=next[2]-intended[2];
    if displacement_x*m.vel[0]+displacement_z*m.vel[2]>0.0 {return false;}
    let Some(wall)=world.surface(wall_id) else {return false;};
    let nx=wall.normal.x;
    let nz=wall.normal.z;
    let ledge_x=next[0]-nx*60.0;
    let ledge_z=next[2]-nz*60.0;
    let Some(ledge)=world.find_floor(ledge_x,next[1]+160.0,ledge_z) else {return false;};
    if ledge.height-next[1]<=100.0 {return false;}
    let Some(ledge_floor)=world.surface(ledge.surface) else {return false;};
    let floor_angle=atan2s(ledge_floor.normal.z,ledge_floor.normal.x);
    m.pos=[ledge_x,ledge.height,ledge_z];
    m.floor=Some(ledge.surface);
    m.floor_height=ledge.height;
    m.floor_angle=floor_angle;
    m.face_angle[0]=0;
    m.face_angle[1]=atan2s(nz,nx).wrapping_add(i16::MIN);
    true
}

pub fn apply_gravity(m:&mut MarioState) {
    if m.action == ACT_TWIRLING && m.vel[1] < 0.0 {
        let heaviness=if m.angle_vel[1] > 1024 { 1024.0 / m.angle_vel[1] as f32 } else { 1.0 };
        let terminal=-75.0*heaviness;
        m.vel[1]-=4.0*heaviness;
        if m.vel[1] < terminal { m.vel[1]=terminal; }
    } else if m.action == ACT_SHOT_FROM_CANNON {
        m.vel[1]-=1.0;
        if m.vel[1] < -75.0 { m.vel[1]=-75.0; }
    } else if m.action == ACT_LONG_JUMP || m.action == ACT_SLIDE_KICK || m.action == ACT_BBH_ENTER_SPIN {
        m.vel[1]-=2.0;
        if m.vel[1] < -75.0 { m.vel[1]=-75.0; }
    } else if m.action == ACT_LAVA_BOOST || m.action == ACT_FALL_AFTER_STAR_GRAB {
        m.vel[1]-=3.2;
        if m.vel[1] < -65.0 { m.vel[1]=-65.0; }
    } else if m.action == ACT_GETTING_BLOWN {
        m.vel[1]-=m.getting_blown_gravity;
        if m.vel[1] < -75.0 { m.vel[1]=-75.0; }
    } else if should_strengthen_gravity_for_jump_ascent(m) {
        m.vel[1]/=4.0;
    } else if m.action & ACT_FLAG_METAL_WATER != 0 {
        m.vel[1]-=1.6;
        if m.vel[1] < -16.0 { m.vel[1]=-16.0; }
    } else {
        m.vel[1]-=4.0;
        if m.vel[1] < -75.0 { m.vel[1]=-75.0; }
    }
}

#[inline]
fn should_strengthen_gravity_for_jump_ascent(m:&MarioState)->bool {
    if m.flags & MARIO_UNKNOWN_08 == 0 { return false; }
    if m.action & (ACT_FLAG_INTANGIBLE | ACT_FLAG_INVULNERABLE) != 0 { return false; }
    m.input & (INPUT_A_DOWN as u16) == 0 && m.vel[1] > 20.0 && m.action & ACT_FLAG_CONTROL_JUMP_HEIGHT != 0
}
