use crate::action::*;
use crate::collision::CollisionWorld;
use crate::mario::MarioState;
use crate::mario_action::set_mario_action;
use crate::math::{approach_f32, approach_i32, coss, sins};
use crate::surface_types::*;

const WATER_CURRENT_SPEEDS:[f32;4]=[28.0,12.0,8.0,4.0];
const MIN_SWIM_SPEED:f32=16.0;
const MIN_SWIM_STRENGTH:i16=160;

#[inline]
fn swimming_near_surface(m:&MarioState)->bool {
    m.flags & MARIO_METAL_CAP == 0 && (m.water_level as f32-80.0)-m.pos[1] < 400.0
}

fn get_buoyancy(m:&MarioState)->f32 {
    if m.flags & MARIO_METAL_CAP != 0 {
        if m.action & ACT_FLAG_INVULNERABLE != 0 {-2.0}else{-18.0}
    } else if swimming_near_surface(m) {
        1.25
    } else if m.action & ACT_FLAG_MOVING == 0 {
        -2.0
    } else {
        0.0
    }
}

pub fn set_water_plunge_action(m:&mut MarioState, world:&CollisionWorld)->bool {
    m.forward_vel/=4.0;
    m.vel[1]/=2.0;
    m.pos[1]=m.water_level as f32-100.0;
    m.face_angle[2]=0;
    m.angle_vel=[0;3];
    if m.action & ACT_FLAG_DIVING == 0 {
        m.face_angle[0]=0;
    }
    set_mario_action(m,world,ACT_WATER_PLUNGE,0)
}

fn apply_water_current(m:&MarioState, world:&CollisionWorld, step:&mut [f32;3]) {
    let Some(floor)=m.floor.and_then(|id|world.surface(id)) else{return;};
    if floor.surface_type as i32 != SURFACE_FLOWING_WATER {return;}
    let angle=(floor.force as i32).wrapping_shl(8) as i16;
    let index=((floor.force as u16)>>8) as usize;
    let speed=*WATER_CURRENT_SPEEDS.get(index).unwrap_or(&0.0);
    step[0]+=speed*sins(angle);
    step[2]+=speed*coss(angle);
}

fn perform_water_full_step(m:&mut MarioState, world:&CollisionWorld, mut next:[f32;3])->u32 {
    let wall=world.resolve_walls(&mut next,10.0,110.0);
    let Some(floor)=world.find_floor(next[0],next[1],next[2]) else{return WATER_STEP_CANCELLED;};
    let ceil=world.find_ceil(next[0],floor.height,next[2]);
    let ceil_height=ceil.map_or(20_000.0,|h|h.height);

    if next[1]>=floor.height {
        if ceil_height-next[1]>=160.0 {
            m.pos=next;
            m.floor=Some(floor.surface);
            m.floor_height=floor.height;
            return if wall.is_some(){WATER_STEP_HIT_WALL}else{WATER_STEP_NONE};
        }
        if ceil_height-floor.height<160.0 {return WATER_STEP_CANCELLED;}
        m.pos=[next[0],ceil_height-160.0,next[2]];
        m.floor=Some(floor.surface);
        m.floor_height=floor.height;
        return WATER_STEP_HIT_CEILING;
    }

    if ceil_height-floor.height<160.0 {return WATER_STEP_CANCELLED;}
    m.pos=[next[0],floor.height,next[2]];
    m.floor=Some(floor.surface);
    m.floor_height=floor.height;
    WATER_STEP_HIT_FLOOR
}

fn perform_water_step(m:&mut MarioState, world:&CollisionWorld)->u32 {
    let mut step=m.vel;
    if m.action & ACT_FLAG_SWIMMING != 0 {
        apply_water_current(m,world,&mut step);
    }
    let mut next=[
        m.pos[0]+step[0],
        m.pos[1]+step[1],
        m.pos[2]+step[2],
    ];
    let surface=m.water_level as f32-80.0;
    if next[1]>surface {
        next[1]=surface;
        m.vel[1]=0.0;
    }
    perform_water_full_step(m,world,next)
}

fn stationary_slow_down(m:&mut MarioState) {
    let buoyancy=get_buoyancy(m);
    m.angle_vel[0]=0;
    m.angle_vel[1]=0;
    m.forward_vel=approach_f32(m.forward_vel,0.0,1.0,1.0);
    m.vel[1]=approach_f32(m.vel[1],buoyancy,2.0,1.0);
    m.face_angle[0]=approach_i32(m.face_angle[0] as i32,0,0x200,0x200) as i16;
    m.face_angle[2]=approach_i32(m.face_angle[2] as i32,0,0x100,0x100) as i16;
    m.vel[0]=m.forward_vel*coss(m.face_angle[0])*sins(m.face_angle[1]);
    m.vel[2]=m.forward_vel*coss(m.face_angle[0])*coss(m.face_angle[1]);
}

fn update_swimming_yaw(m:&mut MarioState) {
    let target=-(10.0*m.stick_x) as i16;
    if target>0 {
        if m.angle_vel[1]<0 {
            m.angle_vel[1]=m.angle_vel[1].wrapping_add(0x40);
            if m.angle_vel[1]>0x10 {m.angle_vel[1]=0x10;}
        } else {
            m.angle_vel[1]=approach_i32(m.angle_vel[1] as i32,target as i32,0x10,0x20) as i16;
        }
    } else if target<0 {
        if m.angle_vel[1]>0 {
            m.angle_vel[1]=m.angle_vel[1].wrapping_sub(0x40);
            if m.angle_vel[1] < -0x10 {m.angle_vel[1]=-0x10;}
        } else {
            m.angle_vel[1]=approach_i32(m.angle_vel[1] as i32,target as i32,0x20,0x10) as i16;
        }
    } else {
        m.angle_vel[1]=approach_i32(m.angle_vel[1] as i32,0,0x40,0x40) as i16;
    }
    m.face_angle[1]=m.face_angle[1].wrapping_add(m.angle_vel[1]);
    m.face_angle[2]=m.angle_vel[1].wrapping_mul(-8);
}

fn update_swimming_pitch(m:&mut MarioState) {
    let target=-(252.0*m.stick_y) as i16;
    let vel=if m.face_angle[0]<0 {0x100}else{0x200};
    if m.face_angle[0]<target {
        m.face_angle[0]=m.face_angle[0].wrapping_add(vel);
        if m.face_angle[0]>target {m.face_angle[0]=target;}
    } else if m.face_angle[0]>target {
        m.face_angle[0]=m.face_angle[0].wrapping_sub(vel);
        if m.face_angle[0]<target {m.face_angle[0]=target;}
    }
}

fn update_swimming_speed(m:&mut MarioState, threshold:f32) {
    let buoyancy=get_buoyancy(m);
    if m.action & ACT_FLAG_STATIONARY != 0 {m.forward_vel-=2.0;}
    if m.forward_vel<0.0 {m.forward_vel=0.0;}
    if m.forward_vel>28.0 {m.forward_vel=28.0;}
    if m.forward_vel>threshold {m.forward_vel-=0.5;}
    m.vel[0]=m.forward_vel*coss(m.face_angle[0])*sins(m.face_angle[1]);
    m.vel[1]=m.forward_vel*sins(m.face_angle[0])+buoyancy;
    m.vel[2]=m.forward_vel*coss(m.face_angle[0])*coss(m.face_angle[1]);
}

fn common_swimming_step(m:&mut MarioState, world:&CollisionWorld, strength:i16) {
    update_swimming_yaw(m);
    update_swimming_pitch(m);
    update_swimming_speed(m,strength as f32/10.0);
    match perform_water_step(m,world) {
        WATER_STEP_HIT_CEILING=>{
            if m.face_angle[0]>-0x3000 {m.face_angle[0]=m.face_angle[0].wrapping_sub(0x100);}
        }
        WATER_STEP_HIT_WALL if m.stick_y==0.0=>{
            if m.face_angle[0]>0 {
                m.face_angle[0]=m.face_angle[0].wrapping_add(0x200).min(0x3F00);
            } else {
                m.face_angle[0]=m.face_angle[0].wrapping_sub(0x200).max(-0x3F00);
            }
        }
        _=>{}
    }
    if m.pos[1]>=m.water_level as f32-130.0 {
        m.particle_flags|=PARTICLE_WAVE_TRAIL;
    }
}

fn check_water_jump(m:&mut MarioState, world:&CollisionWorld)->bool {
    let probe=(m.pos[1]+1.5) as i32;
    if m.input & INPUT_A_PRESSED as u16 != 0
        && probe>=m.water_level as i32-80
        && m.face_angle[0]>=0
        && m.stick_y < -60.0
    {
        m.angle_vel=[0;3];
        m.vel[1]=62.0;
        return set_mario_action(m,world,ACT_WATER_JUMP,0);
    }
    false
}

fn act_water_idle(m:&mut MarioState, world:&CollisionWorld)->bool {
    if m.flags & MARIO_METAL_CAP != 0 {
        return set_mario_action(m,world,ACT_METAL_WATER_FALLING,1);
    }
    if m.input & INPUT_B_PRESSED as u16 != 0 {
        return set_mario_action(m,world,ACT_WATER_PUNCH,0);
    }
    if m.input & INPUT_A_PRESSED as u16 != 0 {
        return set_mario_action(m,world,ACT_BREASTSTROKE,0);
    }
    update_swimming_yaw(m);
    update_swimming_pitch(m);
    update_swimming_speed(m,MIN_SWIM_SPEED);
    perform_water_step(m,world);
    if m.pos[1]>=m.water_level as f32-130.0 {m.particle_flags|=PARTICLE_IDLE_WATER_WAVE;}
    false
}

fn act_breaststroke(m:&mut MarioState, world:&CollisionWorld)->bool {
    if m.action_arg==0 {m.swim_strength=MIN_SWIM_STRENGTH;}
    if m.flags & MARIO_METAL_CAP != 0 {
        return set_mario_action(m,world,ACT_METAL_WATER_FALLING,1);
    }
    if m.input & INPUT_B_PRESSED as u16 != 0 {
        return set_mario_action(m,world,ACT_WATER_PUNCH,0);
    }
    m.action_timer=m.action_timer.wrapping_add(1);
    if m.action_timer==14 {return set_mario_action(m,world,ACT_FLUTTER_KICK,0);}
    if check_water_jump(m,world) {return true;}
    if m.action_timer<6 {m.forward_vel+=0.5;}
    if m.action_timer>=9 {m.forward_vel+=1.5;}
    if m.action_timer>=2 && m.action_timer<6 && m.input & INPUT_A_PRESSED as u16 != 0 {
        m.action_state=1;
    }
    if m.action_timer==9 && m.action_state==1 {
        m.action_state=0;
        m.action_timer=1;
        m.swim_strength=MIN_SWIM_STRENGTH;
    }
    common_swimming_step(m,world,m.swim_strength);
    false
}

fn act_swimming_end(m:&mut MarioState, world:&CollisionWorld)->bool {
    if m.flags & MARIO_METAL_CAP != 0 {
        return set_mario_action(m,world,ACT_METAL_WATER_FALLING,1);
    }
    if m.input & INPUT_B_PRESSED as u16 != 0 {
        return set_mario_action(m,world,ACT_WATER_PUNCH,0);
    }
    if m.action_timer>=15 {return set_mario_action(m,world,ACT_WATER_ACTION_END,0);}
    if check_water_jump(m,world) {return true;}
    if m.input & INPUT_A_DOWN as u16 != 0 && m.action_timer>=7 {
        if m.action_timer==7 && m.swim_strength<280 {m.swim_strength+=10;}
        return set_mario_action(m,world,ACT_BREASTSTROKE,1);
    }
    if m.action_timer>=7 {m.swim_strength=MIN_SWIM_STRENGTH;}
    m.action_timer=m.action_timer.wrapping_add(1);
    m.forward_vel-=0.25;
    common_swimming_step(m,world,m.swim_strength);
    false
}

fn act_flutter_kick(m:&mut MarioState, world:&CollisionWorld)->bool {
    if m.flags & MARIO_METAL_CAP != 0 {
        return set_mario_action(m,world,ACT_METAL_WATER_FALLING,1);
    }
    if m.input & INPUT_B_PRESSED as u16 != 0 {
        return set_mario_action(m,world,ACT_WATER_PUNCH,0);
    }
    if m.input & INPUT_A_DOWN as u16 == 0 {
        if m.action_timer==0 && m.swim_strength<280 {m.swim_strength+=10;}
        return set_mario_action(m,world,ACT_SWIMMING_END,0);
    }
    m.forward_vel=approach_f32(m.forward_vel,12.0,0.1,0.15);
    m.action_timer=1;
    m.swim_strength=MIN_SWIM_STRENGTH;
    common_swimming_step(m,world,m.swim_strength);
    false
}

fn act_water_plunge(m:&mut MarioState, world:&CollisionWorld)->bool {
    let end_v=if swimming_near_surface(m){0.0}else{-5.0};
    let diving=(m.prev_action & ACT_FLAG_DIVING != 0)||(m.input & INPUT_A_DOWN as u16 != 0);
    m.action_timer=m.action_timer.wrapping_add(1);
    stationary_slow_down(m);
    let step=perform_water_step(m,world);
    if m.action_state==0 {
        m.particle_flags|=PARTICLE_WATER_SPLASH;
        m.action_state=1;
    }
    if step==WATER_STEP_HIT_FLOOR || m.vel[1]>=end_v || m.action_timer>20 {
        return set_mario_action(m,world,if diving{ACT_FLUTTER_KICK}else{ACT_WATER_ACTION_END},0);
    }
    m.particle_flags|=PARTICLE_PLUNGE_BUBBLE;
    false
}

fn act_water_action_end(m:&mut MarioState, world:&CollisionWorld)->bool {
    if m.input & INPUT_A_PRESSED as u16 != 0 {
        return set_mario_action(m,world,ACT_BREASTSTROKE,0);
    }
    update_swimming_yaw(m);
    update_swimming_pitch(m);
    update_swimming_speed(m,MIN_SWIM_SPEED);
    perform_water_step(m,world);
    // Animation completion is presentation-owned for now; preserve the action
    // for a short deterministic interval before returning to idle.
    m.action_timer=m.action_timer.wrapping_add(1);
    if m.action_timer>=15 {return set_mario_action(m,world,ACT_WATER_IDLE,0);}
    false
}

pub fn execute_submerged(m:&mut MarioState, world:&CollisionWorld)->bool {
    // Original submerged common cancel: climbing above the water surface returns
    // Mario to the walking group.
    if m.pos[1] > m.water_level as f32-80.0
        && m.action != ACT_WATER_PLUNGE
        && m.action & ACT_FLAG_METAL_WATER == 0
    {
        m.angle_vel=[0;3];
        return set_mario_action(m,world,ACT_WALKING,0);
    }

    match m.action {
        ACT_WATER_IDLE=>act_water_idle(m,world),
        ACT_WATER_PLUNGE=>act_water_plunge(m,world),
        ACT_BREASTSTROKE=>act_breaststroke(m,world),
        ACT_SWIMMING_END=>act_swimming_end(m,world),
        ACT_FLUTTER_KICK=>act_flutter_kick(m,world),
        ACT_WATER_ACTION_END=>act_water_action_end(m,world),
        _=>{
            stationary_slow_down(m);
            perform_water_step(m,world);
            false
        }
    }
}
