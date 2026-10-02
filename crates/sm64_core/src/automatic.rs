use crate::{
    action::*,
    collision::CollisionWorld,
    mario::MarioState,
    mario_action::set_mario_action,
    mario_step::stop_and_set_height_to_floor,
    math::{approach_i32, coss, sins},
    surface_types::SURFACE_HANGABLE,
};

const HANG_NONE:u32=0;
const HANG_HIT_CEIL_OR_OOB:u32=1;
const HANG_LEFT_CEIL:u32=2;

fn let_go_of_ledge(m:&mut MarioState,world:&CollisionWorld)->bool {
    m.vel[1]=0.0;
    m.forward_vel=-8.0;
    m.pos[0]-=60.0*sins(m.face_angle[1]);
    m.pos[2]-=60.0*coss(m.face_angle[1]);

    if let Some(floor)=world.find_floor(m.pos[0],m.pos[1],m.pos[2]) {
        if floor.height < m.pos[1]-100.0 {
            m.pos[1]-=100.0;
        } else {
            m.pos[1]=floor.height;
        }
        m.floor=Some(floor.surface);
        m.floor_height=floor.height;
    } else {
        m.pos[1]-=100.0;
    }
    set_mario_action(m,world,ACT_SOFT_BONK,0)
}

fn climb_up_ledge(m:&mut MarioState) {
    m.pos[0]+=14.0*sins(m.face_angle[1]);
    m.pos[2]+=14.0*coss(m.face_angle[1]);
}

fn floor_height_relative_polar(
    m:&MarioState,
    world:&CollisionWorld,
    angle:i16,
    dist:f32,
)->f32 {
    let yaw=m.face_angle[1].wrapping_add(angle);
    let x=m.pos[0]+dist*sins(yaw);
    let z=m.pos[2]+dist*coss(yaw);
    world.find_floor(x,m.pos[1],z).map_or(-11000.0,|hit|hit.height)
}

fn act_ledge_grab(m:&mut MarioState,world:&CollisionWorld)->bool {
    let intended_dyaw=m.intended_yaw.wrapping_sub(m.face_angle[1]);
    let has_space=m.ceil_height-m.floor_height>=160.0;

    if m.action_timer<10 {m.action_timer+=1;}

    let floor_ok=m.floor
        .and_then(|id|world.surface(id))
        .is_some_and(|floor|floor.normal.y>=0.9063078);
    if !floor_ok {
        return let_go_of_ledge(m,world);
    }

    if m.input & ((INPUT_Z_PRESSED|INPUT_OFF_FLOOR) as u16)!=0 {
        return let_go_of_ledge(m,world);
    }

    if m.input & INPUT_A_PRESSED as u16!=0 && has_space {
        return set_mario_action(m,world,ACT_LEDGE_CLIMB_FAST,0);
    }

    if m.action_timer==10 && m.input & INPUT_NONZERO_ANALOG as u16!=0 {
        if (-0x4000..=0x4000).contains(&(intended_dyaw as i32)) {
            if has_space {
                return set_mario_action(m,world,ACT_LEDGE_CLIMB_SLOW_1,0);
            }
        } else {
            return let_go_of_ledge(m,world);
        }
    }

    let height_above=m.pos[1]-floor_height_relative_polar(m,world,i16::MIN,30.0);
    if has_space && height_above<100.0 {
        return set_mario_action(m,world,ACT_LEDGE_CLIMB_FAST,0);
    }

    stop_and_set_height_to_floor(m);
    false
}

fn act_ledge_climb_slow(m:&mut MarioState,world:&CollisionWorld)->bool {
    if m.input & INPUT_OFF_FLOOR as u16!=0 {
        return let_go_of_ledge(m,world);
    }

    // The original transition is driven by MARIO_ANIM_SLOW_LEDGE_GRAB.
    // Until animation playback is ported, preserve its two-phase action shape
    // with explicit simulation-frame boundaries.
    m.action_timer=m.action_timer.wrapping_add(1);
    if m.action_timer==17 {
        m.action=ACT_LEDGE_CLIMB_SLOW_2;
    }
    if m.action_timer>=34 {
        climb_up_ledge(m);
        return set_mario_action(m,world,ACT_IDLE,0);
    }

    stop_and_set_height_to_floor(m);
    false
}

fn act_ledge_climb_fast(m:&mut MarioState,world:&CollisionWorld)->bool {
    if m.input & INPUT_OFF_FLOOR as u16!=0 {
        return let_go_of_ledge(m,world);
    }
    m.action_timer=m.action_timer.wrapping_add(1);
    stop_and_set_height_to_floor(m);

    // Temporary duration until MARIO_ANIM_FAST_LEDGE_GRAB is translated.
    if m.action_timer>=12 {
        climb_up_ledge(m);
        return set_mario_action(m,world,ACT_IDLE,0);
    }
    false
}

fn act_ledge_climb_down(m:&mut MarioState,world:&CollisionWorld)->bool {
    if m.input & INPUT_OFF_FLOOR as u16!=0 {
        return let_go_of_ledge(m,world);
    }
    m.action_timer=m.action_timer.wrapping_add(1);
    stop_and_set_height_to_floor(m);
    if m.action_timer>=20 {
        return set_mario_action(m,world,ACT_LEDGE_GRAB,1);
    }
    false
}

fn perform_hanging_step(
    m:&mut MarioState,
    world:&CollisionWorld,
    mut next:[f32;3],
)->u32 {
    m.wall=world.resolve_walls(&mut next,50.0,50.0);
    let Some(floor)=world.find_floor(next[0],next[1],next[2]) else {
        return HANG_HIT_CEIL_OR_OOB;
    };
    let Some(ceil)=world.find_ceil(next[0],floor.height,next[2]) else {
        return HANG_LEFT_CEIL;
    };
    if ceil.height-floor.height<=160.0 {
        return HANG_HIT_CEIL_OR_OOB;
    }
    if world.surface(ceil.surface).is_none_or(|surface|surface.surface_type as i32!=SURFACE_HANGABLE) {
        return HANG_LEFT_CEIL;
    }

    let ceil_offset=ceil.height-(next[1]+160.0);
    if ceil_offset< -30.0 {
        return HANG_HIT_CEIL_OR_OOB;
    }
    if ceil_offset>30.0 {
        return HANG_LEFT_CEIL;
    }

    // Preserve the original quirk: snap to the previous ceiling height here,
    // not the newly found one.
    next[1]=m.ceil_height-160.0;
    m.pos=next;
    m.floor=Some(floor.surface);
    m.floor_height=floor.height;
    m.ceil=Some(ceil.surface);
    m.ceil_height=ceil.height;
    HANG_NONE
}

fn update_hang_stationary(m:&mut MarioState) {
    m.forward_vel=0.0;
    m.slide_vel_x=0.0;
    m.slide_vel_z=0.0;
    m.pos[1]=m.ceil_height-160.0;
    m.vel=[0.0;3];
}

fn update_hang_moving(m:&mut MarioState,world:&CollisionWorld)->u32 {
    m.forward_vel=(m.forward_vel+1.0).min(4.0);
    let delta=m.intended_yaw.wrapping_sub(m.face_angle[1]) as i32;
    m.face_angle[1]=m.intended_yaw
        .wrapping_sub(approach_i32(delta,0,0x800,0x800) as i16);

    m.slide_yaw=m.face_angle[1];
    m.slide_vel_x=m.forward_vel*sins(m.face_angle[1]);
    m.slide_vel_z=m.forward_vel*coss(m.face_angle[1]);
    m.vel=[m.slide_vel_x,0.0,m.slide_vel_z];

    let ceil_normal_y=m.ceil
        .and_then(|id|world.surface(id))
        .map_or(-1.0,|ceil|ceil.normal.y);
    let next=[
        m.pos[0]-ceil_normal_y*m.vel[0],
        m.pos[1],
        m.pos[2]-ceil_normal_y*m.vel[2],
    ];
    perform_hanging_step(m,world,next)
}

fn hanging_common_cancel(m:&mut MarioState,world:&CollisionWorld)->Option<bool> {
    if m.input & INPUT_A_DOWN as u16==0 {
        return Some(set_mario_action(m,world,ACT_FREEFALL,0));
    }
    if m.input & INPUT_Z_PRESSED as u16!=0 {
        return Some(set_mario_action(m,world,ACT_GROUND_POUND,0));
    }
    let hangable=m.ceil
        .and_then(|id|world.surface(id))
        .is_some_and(|ceil|ceil.surface_type as i32==SURFACE_HANGABLE);
    if !hangable {
        return Some(set_mario_action(m,world,ACT_FREEFALL,0));
    }
    None
}

fn act_start_hanging(m:&mut MarioState,world:&CollisionWorld)->bool {
    m.action_timer=m.action_timer.wrapping_add(1);
    if m.input & INPUT_NONZERO_ANALOG as u16!=0 && m.action_timer>=31 {
        return set_mario_action(m,world,ACT_HANGING,0);
    }
    if let Some(result)=hanging_common_cancel(m,world) {return result;}
    update_hang_stationary(m);
    // Original completion is animation-driven. 31 frames is retained as the
    // temporary boundary already used by its input shortcut.
    if m.action_timer>=31 {
        return set_mario_action(m,world,ACT_HANGING,0);
    }
    false
}

fn act_hanging(m:&mut MarioState,world:&CollisionWorld)->bool {
    if m.input & INPUT_NONZERO_ANALOG as u16!=0 {
        return set_mario_action(m,world,ACT_HANG_MOVING,m.action_arg);
    }
    if let Some(result)=hanging_common_cancel(m,world) {return result;}
    update_hang_stationary(m);
    false
}

fn act_hang_moving(m:&mut MarioState,world:&CollisionWorld)->bool {
    if let Some(result)=hanging_common_cancel(m,world) {return result;}
    if update_hang_moving(m,world)==HANG_LEFT_CEIL {
        return set_mario_action(m,world,ACT_FREEFALL,0);
    }
    if m.input & INPUT_UNKNOWN_5 as u16!=0 {
        m.action_arg^=1;
        return set_mario_action(m,world,ACT_HANGING,m.action_arg);
    }
    false
}

pub fn execute_automatic(m:&mut MarioState,world:&CollisionWorld)->bool {
    if m.pos[1]<m.water_level as f32-100.0 {
        return crate::submerged::set_water_plunge_action(m,world);
    }

    match m.action {
        ACT_LEDGE_GRAB=>act_ledge_grab(m,world),
        ACT_LEDGE_CLIMB_SLOW_1|ACT_LEDGE_CLIMB_SLOW_2=>act_ledge_climb_slow(m,world),
        ACT_LEDGE_CLIMB_DOWN=>act_ledge_climb_down(m,world),
        ACT_LEDGE_CLIMB_FAST=>act_ledge_climb_fast(m,world),
        ACT_START_HANGING=>act_start_hanging(m,world),
        ACT_HANGING=>act_hanging(m,world),
        ACT_HANG_MOVING=>act_hang_moving(m,world),
        _=>false,
    }
}
