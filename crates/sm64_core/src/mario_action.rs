use crate::action::*;
use crate::collision::CollisionWorld;
use crate::mario::MarioState;
use crate::mario_step::mario_set_forward_vel;
use crate::surface_types::*;

#[inline]
fn set_y_vel_based_on_fspeed(m:&mut MarioState, initial:f32, multiplier:f32) {
    m.vel[1]=initial + m.forward_vel*multiplier;
    if m.squish_timer != 0 || m.quicksand_depth > 1.0 { m.vel[1]*=0.5; }
}

fn setup_airborne(m:&mut MarioState, mut action:u32, action_arg:u32)->u32 {
    if (m.squish_timer != 0 || m.quicksand_depth >= 1.0) && (action==ACT_DOUBLE_JUMP || action==ACT_TWIRLING) {
        action=ACT_JUMP;
    }
    match action {
        ACT_DOUBLE_JUMP => { set_y_vel_based_on_fspeed(m,52.0,0.25); m.forward_vel*=0.8; }
        ACT_BACKFLIP => { m.forward_vel=-16.0; set_y_vel_based_on_fspeed(m,62.0,0.0); }
        ACT_TRIPLE_JUMP => { set_y_vel_based_on_fspeed(m,69.0,0.0); m.forward_vel*=0.8; }
        ACT_FLYING_TRIPLE_JUMP => set_y_vel_based_on_fspeed(m,82.0,0.0),
        ACT_WATER_JUMP | ACT_HOLD_WATER_JUMP if action_arg==0 => set_y_vel_based_on_fspeed(m,42.0,0.0),
        ACT_BURNING_JUMP => { m.vel[1]=31.5; m.forward_vel=8.0; }
        ACT_RIDING_SHELL_JUMP => set_y_vel_based_on_fspeed(m,42.0,0.25),
        ACT_JUMP | ACT_HOLD_JUMP => { set_y_vel_based_on_fspeed(m,42.0,0.25); m.forward_vel*=0.8; }
        ACT_WALL_KICK_AIR | ACT_TOP_OF_POLE_JUMP => {
            set_y_vel_based_on_fspeed(m,62.0,0.0);
            if m.forward_vel < 24.0 { m.forward_vel=24.0; }
            m.wall_kick_timer=0;
        }
        ACT_SIDE_FLIP => { set_y_vel_based_on_fspeed(m,62.0,0.0); m.forward_vel=8.0; m.face_angle[1]=m.intended_yaw; }
        ACT_STEEP_JUMP => { set_y_vel_based_on_fspeed(m,42.0,0.25); m.face_angle[0]=-0x2000; }
        ACT_LAVA_BOOST => { m.vel[1]=84.0; if action_arg==0 { m.forward_vel=0.0; } }
        ACT_DIVE => { mario_set_forward_vel(m,(m.forward_vel+15.0).min(48.0)); }
        ACT_LONG_JUMP => {
            set_y_vel_based_on_fspeed(m,30.0,0.0);
            m.forward_vel*=1.5;
            if m.forward_vel>48.0 { m.forward_vel=48.0; }
        }
        ACT_SLIDE_KICK => { m.vel[1]=12.0; if m.forward_vel<32.0 {m.forward_vel=32.0;} }
        ACT_JUMP_KICK => m.vel[1]=20.0,
        _ => {}
    }
    m.peak_height=m.pos[1];
    m.flags |= MARIO_UNKNOWN_08;
    action
}

fn setup_moving(m:&mut MarioState, world:&CollisionWorld, action:u32)->u32 {
    if action==ACT_WALKING {
        let mag=m.intended_mag.min(8.0);
        if crate::surface_props::mario_get_floor_class(m,world)!=SURFACE_CLASS_VERY_SLIPPERY && m.forward_vel>=0.0 && m.forward_vel<mag {
            m.forward_vel=mag;
        }
    } else if action==ACT_HOLD_WALKING {
        let mag=m.intended_mag.min(8.0);
        if m.forward_vel>=0.0 && m.forward_vel<mag/2.0 { m.forward_vel=mag/2.0; }
    }
    action
}

fn setup_submerged(m:&mut MarioState, action:u32)->u32 {
    if action==ACT_METAL_WATER_JUMP || action==ACT_HOLD_METAL_WATER_JUMP { m.vel[1]=32.0; }
    action
}

fn setup_cutscene(m:&mut MarioState, action:u32)->u32 {
    match action {
        ACT_EMERGE_FROM_PIPE => m.vel[1]=52.0,
        ACT_FALL_AFTER_STAR_GRAB => mario_set_forward_vel(m,0.0),
        ACT_SPAWN_SPIN_AIRBORNE => mario_set_forward_vel(m,2.0),
        ACT_SPECIAL_EXIT_AIRBORNE | ACT_SPECIAL_DEATH_EXIT => m.vel[1]=64.0,
        _ => {}
    }
    action
}

pub fn set_mario_action(m:&mut MarioState, world:&CollisionWorld, mut action:u32, action_arg:u32)->bool {
    action=match action & ACT_GROUP_MASK {
        ACT_GROUP_MOVING => setup_moving(m,world,action),
        ACT_GROUP_AIRBORNE => setup_airborne(m,action,action_arg),
        ACT_GROUP_SUBMERGED => setup_submerged(m,action),
        ACT_GROUP_CUTSCENE => setup_cutscene(m,action),
        _ => action,
    };
    m.flags &= !(MARIO_ACTION_SOUND_PLAYED | MARIO_MARIO_SOUND_PLAYED);
    if m.action & ACT_FLAG_AIR == 0 { m.flags &= !MARIO_UNKNOWN_18; }
    m.prev_action=m.action;
    m.action=action;
    m.action_arg=action_arg;
    m.action_state=0;
    m.action_timer=0;
    true
}
