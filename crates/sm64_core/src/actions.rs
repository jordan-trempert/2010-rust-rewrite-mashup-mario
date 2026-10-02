use crate::action::*;
use crate::collision::CollisionWorld;
use crate::mario::MarioState;
use crate::mario_action::set_mario_action;
use crate::mario_step::{mario_set_forward_vel, perform_air_step, perform_ground_step, stationary_ground_step};
use crate::math::{approach_f32, approach_i32, coss, sins};
use crate::ground_motion::{analog_stick_held_back, apply_slope_accel, should_begin_sliding, update_sliding};

pub fn execute_mario_action(m:&mut MarioState, world:&CollisionWorld) {
    if m.action==ACT_UNINITIALIZED { return; }
    let mut in_loop=true;
    let mut guard=0;
    while in_loop && guard<32 {
        guard+=1;
        let group=m.action & ACT_GROUP_MASK;
        if matches!(group,ACT_GROUP_STATIONARY|ACT_GROUP_MOVING|ACT_GROUP_AIRBORNE)
            && m.pos[1] < m.water_level as f32-100.0
        {
            in_loop=crate::submerged::set_water_plunge_action(m,world);
            continue;
        }
        in_loop=match group {
            ACT_GROUP_STATIONARY => execute_stationary(m,world),
            ACT_GROUP_MOVING => execute_moving(m,world),
            ACT_GROUP_AIRBORNE => execute_airborne(m,world),
            ACT_GROUP_SUBMERGED => crate::submerged::execute_submerged(m,world),
            _ => false,
        };
    }
}

fn execute_stationary(m:&mut MarioState, world:&CollisionWorld)->bool {
    match m.action {
        ACT_IDLE => act_idle(m,world),
        ACT_CROUCHING | ACT_START_CROUCHING | ACT_STOP_CROUCHING => {
            stationary_ground_step(m); false
        }
        _ => { stationary_ground_step(m); false }
    }
}

fn act_idle(m:&mut MarioState, world:&CollisionWorld)->bool {
    if m.quicksand_depth>30.0 { return set_mario_action(m,world,ACT_IN_QUICKSAND,0); }
    if m.health<0x300 && m.action_arg&1==0 { return set_mario_action(m,world,ACT_PANTING,0); }
    if m.input & INPUT_A_PRESSED as u16 != 0 { return set_mario_action(m,world,ACT_JUMP,0); }
    if m.input & INPUT_OFF_FLOOR as u16 != 0 { return set_mario_action(m,world,ACT_FREEFALL,0); }
    if m.input & INPUT_NONZERO_ANALOG as u16 != 0 {
        m.face_angle[1]=m.intended_yaw;
        return set_mario_action(m,world,ACT_WALKING,0);
    }
    if m.input & INPUT_B_PRESSED as u16 != 0 { return set_mario_action(m,world,ACT_PUNCHING,0); }
    if m.input & INPUT_Z_DOWN as u16 != 0 { return set_mario_action(m,world,ACT_START_CROUCHING,0); }
    stationary_ground_step(m);
    false
}

fn execute_moving(m:&mut MarioState, world:&CollisionWorld)->bool {
    match m.action {
        ACT_WALKING => act_walking(m,world),
        ACT_DECELERATING => act_decelerating(m,world),
        ACT_BEGIN_SLIDING => act_begin_sliding(m,world,false),
        ACT_CROUCH_SLIDE => act_crouch_slide(m,world),
        ACT_BUTT_SLIDE => act_butt_slide(m,world),
        _ => {
            perform_ground_step(m,world);
            false
        }
    }
}

fn update_walking_speed(m:&mut MarioState, world:&CollisionWorld) {
    let max_target=if m.floor.and_then(|id|world.surface(id)).is_some_and(|f| f.surface_type as i32==crate::SURFACE_SLOW) {24.0}else{32.0};
    let mut target=m.intended_mag.min(max_target);
    if m.quicksand_depth>10.0 { target*=6.25/m.quicksand_depth; }
    if m.forward_vel<=0.0 { m.forward_vel+=1.1; }
    else if m.forward_vel<=target { m.forward_vel+=1.1-m.forward_vel/43.0; }
    else if m.floor.and_then(|id|world.surface(id)).is_some_and(|f|f.normal.y>=0.95) {m.forward_vel-=1.0;}
    if m.forward_vel>48.0 {m.forward_vel=48.0;}
    let delta=m.intended_yaw.wrapping_sub(m.face_angle[1]) as i32;
    let approached=approach_i32(delta,0,0x800,0x800) as i16;
    m.face_angle[1]=m.intended_yaw.wrapping_sub(approached);
    apply_slope_accel(m,world);
}

fn act_walking(m:&mut MarioState, world:&CollisionWorld)->bool {
    if should_begin_sliding(m,world) {return set_mario_action(m,world,ACT_BEGIN_SLIDING,0);}
    if m.input & INPUT_A_PRESSED as u16 != 0 { return set_mario_action(m,world,ACT_JUMP,0); }
    if m.input & INPUT_UNKNOWN_5 as u16 != 0 { return set_mario_action(m,world,ACT_DECELERATING,0); }
    if analog_stick_held_back(m) && m.forward_vel>=16.0 {
        return set_mario_action(m,world,ACT_TURNING_AROUND,0);
    }
    if m.input & INPUT_Z_PRESSED as u16 != 0 { return set_mario_action(m,world,ACT_CROUCH_SLIDE,0); }
    m.action_state=0;
    update_walking_speed(m,world);
    match perform_ground_step(m,world) {
        GROUND_STEP_LEFT_GROUND => { set_mario_action(m,world,ACT_FREEFALL,0); }
        GROUND_STEP_NONE => {
            if m.intended_mag-m.forward_vel>16.0 { m.particle_flags|=PARTICLE_DUST; }
        }
        GROUND_STEP_HIT_WALL => { m.action_timer=0; }
        _=>{}
    }
    false
}

fn act_decelerating(m:&mut MarioState, world:&CollisionWorld)->bool {
    if m.input & INPUT_A_PRESSED as u16 != 0 { return set_mario_action(m,world,ACT_JUMP,0); }
    if m.input & INPUT_NONZERO_ANALOG as u16 != 0 {
        m.face_angle[1]=m.intended_yaw;
        return set_mario_action(m,world,ACT_WALKING,0);
    }
    m.forward_vel=approach_f32(m.forward_vel,0.0,1.0,1.0);
    mario_set_forward_vel(m,m.forward_vel);
    if m.forward_vel==0.0 { return set_mario_action(m,world,ACT_IDLE,0); }
    if perform_ground_step(m,world)==GROUND_STEP_LEFT_GROUND { set_mario_action(m,world,ACT_FREEFALL,0); }
    false
}

fn execute_airborne(m:&mut MarioState, world:&CollisionWorld)->bool {
    match m.action {
        ACT_JUMP => act_common_air(m,world,ACT_JUMP_LAND,AIR_STEP_CHECK_LEDGE_GRAB|AIR_STEP_CHECK_HANG),
        ACT_DOUBLE_JUMP => act_common_air(m,world,ACT_DOUBLE_JUMP_LAND,AIR_STEP_CHECK_LEDGE_GRAB|AIR_STEP_CHECK_HANG),
        ACT_TRIPLE_JUMP => act_common_air(m,world,ACT_TRIPLE_JUMP_LAND,0),
        ACT_BACKFLIP => act_common_air(m,world,ACT_BACKFLIP_LAND,0),
        ACT_FREEFALL => act_common_air(m,world,ACT_FREEFALL_LAND,AIR_STEP_CHECK_LEDGE_GRAB),
        ACT_SIDE_FLIP => act_common_air(m,world,ACT_SIDE_FLIP_LAND,AIR_STEP_CHECK_LEDGE_GRAB),
        ACT_LONG_JUMP => act_common_air(m,world,ACT_LONG_JUMP_LAND,AIR_STEP_CHECK_LEDGE_GRAB),
        _ => { update_air_without_turn(m); perform_air_step(m,world,0); false }
    }
}

fn update_air_without_turn(m:&mut MarioState) {
    let mut sideways=0.0;
    let drag=if m.action==ACT_LONG_JUMP {48.0}else{32.0};
    m.forward_vel=approach_f32(m.forward_vel,0.0,0.35,0.35);
    if m.input & INPUT_NONZERO_ANALOG as u16 != 0 {
        let dyaw=m.intended_yaw.wrapping_sub(m.face_angle[1]);
        let mag=m.intended_mag/32.0;
        m.forward_vel += mag*coss(dyaw)*1.5;
        sideways=mag*sins(dyaw)*10.0;
    }
    if m.forward_vel>drag {m.forward_vel-=1.0;}
    if m.forward_vel < -16.0 {m.forward_vel+=2.0;}
    m.slide_vel_x=m.forward_vel*sins(m.face_angle[1]);
    m.slide_vel_z=m.forward_vel*coss(m.face_angle[1]);
    let side_yaw=m.face_angle[1].wrapping_add(0x4000);
    m.slide_vel_x+=sideways*sins(side_yaw);
    m.slide_vel_z+=sideways*coss(side_yaw);
    m.vel[0]=m.slide_vel_x; m.vel[2]=m.slide_vel_z;
}

fn act_common_air(m:&mut MarioState, world:&CollisionWorld, land_action:u32, step_arg:u32)->bool {
    if m.input & INPUT_Z_PRESSED as u16 != 0 && m.action!=ACT_LONG_JUMP {
        return set_mario_action(m,world,ACT_GROUND_POUND,0);
    }
    update_air_without_turn(m);
    match perform_air_step(m,world,step_arg) {
        AIR_STEP_LANDED => { set_mario_action(m,world,land_action,0); }
        AIR_STEP_HIT_WALL => {
            if m.forward_vel>16.0 {
                if let Some(wall)=m.wall.and_then(|id|world.surface(id)) {
                    let wall_angle=crate::math::atan2s(wall.normal.z,wall.normal.x);
                    m.face_angle[1]=wall_angle.wrapping_sub(m.face_angle[1].wrapping_sub(wall_angle));
                    m.face_angle[1]=m.face_angle[1].wrapping_add(i16::MIN);
                    set_mario_action(m,world,ACT_AIR_HIT_WALL,0);
                }
            } else { mario_set_forward_vel(m,0.0); }
        }
        AIR_STEP_GRABBED_CEILING => { set_mario_action(m,world,ACT_START_HANGING,0); }
        _=>{}
    }
    false
}

fn act_begin_sliding(m:&mut MarioState, world:&CollisionWorld, holding:bool)->bool {
    let action=if crate::surface_props::mario_facing_downhill(m,false) {
        if holding {ACT_HOLD_BUTT_SLIDE}else{ACT_BUTT_SLIDE}
    } else if holding {ACT_HOLD_STOMACH_SLIDE}else{ACT_STOMACH_SLIDE};
    set_mario_action(m,world,action,0)
}

fn act_crouch_slide(m:&mut MarioState, world:&CollisionWorld)->bool {
    if m.input & INPUT_A_PRESSED as u16 != 0 {
        return set_mario_action(m,world,ACT_LONG_JUMP,0);
    }
    if m.input & INPUT_Z_DOWN as u16 == 0 {
        return set_mario_action(m,world,ACT_WALKING,0);
    }
    if update_sliding(m,world,4.0) {
        return set_mario_action(m,world,ACT_CROUCHING,0);
    }
    if perform_ground_step(m,world)==GROUND_STEP_LEFT_GROUND {
        return set_mario_action(m,world,ACT_FREEFALL,0);
    }
    false
}

fn act_butt_slide(m:&mut MarioState, world:&CollisionWorld)->bool {
    if m.input & INPUT_A_PRESSED as u16 != 0 {
        return set_mario_action(m,world,ACT_BUTT_SLIDE_AIR,0);
    }
    if m.input & INPUT_B_PRESSED as u16 != 0 {
        return set_mario_action(m,world,ACT_STOMACH_SLIDE,0);
    }
    if update_sliding(m,world,4.0) {
        return set_mario_action(m,world,ACT_BUTT_SLIDE_STOP,0);
    }
    match perform_ground_step(m,world) {
        GROUND_STEP_LEFT_GROUND=>{set_mario_action(m,world,ACT_BUTT_SLIDE_AIR,0);},
        GROUND_STEP_HIT_WALL=>{
            mario_set_forward_vel(m,-m.forward_vel);
            set_mario_action(m,world,ACT_GROUND_BONK,0);
        },
        _=>{}
    }
    false
}
