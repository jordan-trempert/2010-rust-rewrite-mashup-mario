use sm64_core::{
    CollisionWorld, MarioState, ObjectId, ObjectList, Sm64Object,
    ACT_FLAG_SHORT_HITBOX, INTERACT_COIN, INT_STATUS_INTERACTED,
};

pub const SM64_TICK_HZ: u32 = 30;
pub const SM64_TICK_SECONDS: f64 = 1.0 / SM64_TICK_HZ as f64;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sm64Input { pub stick_x:i8,pub stick_y:i8,pub camera_yaw:i16,pub button_a:bool,pub button_b:bool,pub button_z:bool,pub button_start:bool }
#[derive(Clone, Debug, PartialEq)] pub struct Sm64Snapshot { pub tick:u64,pub mario:MarioState,pub objects:Vec<Sm64Object> }
#[derive(Clone, Debug)] pub struct Sm64World { pub tick:u64,pub mario:MarioState,pub collision:CollisionWorld,pub objects:Vec<Sm64Object>,previous_input:Sm64Input,next_object_id:u32 }
impl Default for Sm64World { fn default()->Self{Self{tick:0,mario:MarioState::default(),collision:CollisionWorld::default(),objects:Vec::new(),previous_input:Sm64Input::default(),next_object_id:1}} }

impl Sm64World {
 pub fn new()->Self{Self::default()}
 pub fn clear_objects(&mut self){self.objects.clear();self.next_object_id=1;}
 pub fn spawn_object(&mut self,model:impl Into<String>,behavior:impl Into<String>,object_list:ObjectList,pos:[f32;3],face_angle:[i16;3])->ObjectId{let id=ObjectId(self.next_object_id);self.next_object_id=self.next_object_id.wrapping_add(1).max(1);self.objects.push(Sm64Object::new(id,model,behavior,object_list,pos,face_angle));id}
 pub fn object(&self,id:ObjectId)->Option<&Sm64Object>{self.objects.iter().find(|o|o.id==id)}
 pub fn object_mut(&mut self,id:ObjectId)->Option<&mut Sm64Object>{self.objects.iter_mut().find(|o|o.id==id)}
 pub fn spawn_mario(&mut self,pos:[f32;3],yaw:i16){use sm64_core::*;self.mario=MarioState::default();self.mario.pos=pos;self.mario.face_angle=[0,yaw,0];self.mario.flags=MARIO_NORMAL_CAP|MARIO_CAP_ON_HEAD;self.mario.frames_since_a=0xFF;self.mario.frames_since_b=0xFF;self.refresh_geometry();if self.mario.floor.is_some(){if self.mario.pos[1]<self.mario.floor_height{self.mario.pos[1]=self.mario.floor_height;}let initial=if self.mario.pos[1]<=self.mario.water_level as f32-100.0{ACT_WATER_IDLE}else{ACT_IDLE};sm64_core::mario_action::set_mario_action(&mut self.mario,&self.collision,initial,0);}}
 pub fn snapshot(&self)->Sm64Snapshot{Sm64Snapshot{tick:self.tick,mario:self.mario.clone(),objects:self.objects.iter().filter(|o|o.active).cloned().collect()}}
 pub fn step(&mut self,input:Sm64Input)->Sm64Snapshot{self.tick=self.tick.wrapping_add(1);self.mario.global_timer=self.tick as u32;self.update_object_metrics();self.update_object_lists_before_player();self.update_inputs(input);self.detect_mario_object_collisions();self.process_mario_object_interactions();if self.mario.floor.is_some(){sm64_core::actions::execute_mario_action(&mut self.mario,&self.collision);}self.update_object_lists_after_player();self.finish_object_frame();self.objects.retain(|o|o.active);self.previous_input=input;self.snapshot()}
 pub fn step_external_player(&mut self,pos:[f32;3],vel:[f32;3],yaw:i16)->Sm64Snapshot{self.tick=self.tick.wrapping_add(1);self.mario.global_timer=self.tick as u32;self.mario.pos=pos;self.mario.vel=vel;self.mario.face_angle[1]=yaw;self.mario.forward_vel=(vel[0]*vel[0]+vel[2]*vel[2]).sqrt();self.refresh_geometry();self.update_object_metrics();self.update_object_lists_before_player();self.detect_mario_object_collisions();self.process_mario_object_interactions();self.update_object_lists_after_player();self.finish_object_frame();self.objects.retain(|o|o.active);self.snapshot()}
 fn update_object_metrics(&mut self){let p=self.mario.pos;for o in &mut self.objects{if o.active{o.begin_frame(p);o.refresh_floor(&self.collision);}}}
 fn update_object_lists_before_player(&mut self){for l in [ObjectList::Spawner,ObjectList::Surface,ObjectList::PoleLike]{self.update_object_list(l);}}
 fn update_object_lists_after_player(&mut self){for l in [ObjectList::Pushable,ObjectList::GenActor,ObjectList::Destructive,ObjectList::Level,ObjectList::Default,ObjectList::Unimportant]{self.update_object_list(l);}}
 fn update_object_list(&mut self,list:ObjectList){let indices=self.objects.iter().enumerate().filter_map(|(i,o)|(o.active&&o.object_list==list).then_some(i)).collect::<Vec<_>>();for i in indices{self.update_object_behavior(i);}}
 fn update_object_behavior(&mut self,index:usize){let behavior=self.objects[index].behavior.clone();match behavior.as_str(){"bhvYellowCoin"|"bhvYellowCoinInit"|"bhvCoinFormationSpawn"=>self.update_yellow_coin(index),"bhvCoinFormation"=>self.update_coin_formation(index),"bhvGoomba"=>self.update_goomba(index),_=>{}}}

 fn update_goomba(&mut self,index:usize){
  const WALK:i32=0; const ATTACKED:i32=1; const JUMP:i32=2;
  let o=&mut self.objects[index];
  // behavior_f32[0]=scale, [1]=relative speed; behavior_i32[0]=target yaw.
  if o.timer==0 && o.behavior_f32[0]==0.0 {
   let size=o.behavior_params_2nd_byte&3;
   let scale=match size{1=>3.5,2=>0.5,_=>1.5};
   o.behavior_f32[0]=scale;o.behavior_f32[1]=4.0/3.0;
   o.hitbox_radius=72.0;o.hitbox_height=50.0;o.hurtbox_radius=42.0;o.hurtbox_height=40.0;
   o.damage_or_coin_value=match size{1=>2,2=>0,_=>1};o.num_loot_coins=1;o.drawing_distance=if size==2{1500.0}else{4000.0};
   o.gravity=-8.0/3.0*scale;o.friction=0.8;o.behavior_i32[0]=o.move_angle[1] as i32;
  }
  let scale=o.behavior_f32[0].max(0.5);
  match o.action {
   WALK=>{
    let target_speed=o.behavior_f32[1]*scale;
    o.forward_vel=approach_f32(o.forward_vel,target_speed,0.4);
    if o.distance_to_mario<500.0 {
     if o.behavior_f32[1]<=2.0 {o.set_action(JUMP);o.forward_vel=0.0;o.vel[1]=50.0/3.0*scale;}
     o.behavior_i32[0]=o.angle_to_mario as i32;o.behavior_f32[1]=20.0;
    } else {
     o.behavior_f32[1]=4.0/3.0;
     if o.timer%120==0 {o.behavior_i32[0]=o.angle_to_home as i32;}
    }
    sm64_core::rotate_yaw_toward(o,o.behavior_i32[0] as i16,0x200);
   }
   ATTACKED=>{o.set_action(JUMP);o.forward_vel=0.0;o.vel[1]=50.0/3.0*scale;o.behavior_i32[0]=o.angle_to_mario as i32;}
   JUMP=>{if o.move_flags&sm64_core::OBJ_MOVE_MASK_ON_GROUND!=0{o.set_action(WALK);}else{sm64_core::rotate_yaw_toward(o,o.behavior_i32[0] as i16,0x800);}}
   _=>o.set_action(WALK),
  }
  sm64_core::object_step(o,&self.collision);
 }

 fn update_coin_formation(&mut self,index:usize){let(action,distance,parent_id,parent_pos,parent_yaw,param_expr)={let o=&self.objects[index];(o.action,o.distance_to_mario,o.id,o.pos,o.face_angle[1],o.behavior_params_expr.clone())};match action{0 if distance<2000.0=>{let formation=coin_formation_kind(&param_expr);let flying=param_expr.contains("FLYING");let mut children=Vec::new();for i in 0..8i32{let Some((relative,mut ground))=coin_formation_offset(formation,i)else{continue};if flying{ground=false;}let cos=sm64_core::math::coss(parent_yaw);let sin=sm64_core::math::sins(parent_yaw);children.push((i,[parent_pos[0]+relative[0]*cos+relative[2]*sin,parent_pos[1]+relative[1],parent_pos[2]-relative[0]*sin+relative[2]*cos],ground));}self.objects[index].set_action(1);for(i,pos,ground)in children{let id=self.spawn_object("MODEL_YELLOW_COIN","bhvCoinFormationSpawn",ObjectList::Level,pos,[0,parent_yaw,0]);if let Some(c)=self.object_mut(id){c.parent=Some(parent_id);c.behavior_params=(i as u32)<<16;c.behavior_params_2nd_byte=i as u8;c.behavior_i32[0]=i32::from(ground);}}}1 if distance>2100.0=>self.objects[index].set_action(2),2=>self.objects[index].set_action(0),_=>{}}}
 fn update_yellow_coin(&mut self,index:usize){let o=&mut self.objects[index];if o.timer==0{o.interact_type=INTERACT_COIN;o.damage_or_coin_value=1;o.hitbox_radius=100.0;o.hitbox_height=64.0;if o.behavior=="bhvCoinFormationSpawn"&&o.behavior_i32[0]!=0{o.pos[1]+=300.0;o.refresh_floor(&self.collision);if o.floor_height< -10000.0||o.pos[1]<o.floor_height{o.active=false;return;}o.pos[1]=o.floor_height;}}o.anim_state=o.anim_state.wrapping_add(1);if o.interact_status&INT_STATUS_INTERACTED!=0{o.active=false;}else{o.interact_status=0;}}
 fn detect_mario_object_collisions(&mut self){self.mario.collided_obj_interact_types=0;let h=if self.mario.action&ACT_FLAG_SHORT_HITBOX!=0{100.0}else{160.0};for o in &mut self.objects{if o.active&&o.tangible&&o.hitbox_radius>0.0&&o.hitbox_height>0.0&&hitboxes_overlap(self.mario.pos,37.0,h,0.0,o.pos,o.hitbox_radius,o.hitbox_height,0.0){self.mario.collided_obj_interact_types|=o.interact_type;}}}
 fn process_mario_object_interactions(&mut self){if self.mario.collided_obj_interact_types&INTERACT_COIN!=0{let p=self.mario.pos;let h=if self.mario.action&ACT_FLAG_SHORT_HITBOX!=0{100.0}else{160.0};if let Some(o)=self.objects.iter_mut().find(|o|o.active&&o.interact_type&INTERACT_COIN!=0&&hitboxes_overlap(p,37.0,h,0.0,o.pos,o.hitbox_radius,o.hitbox_height,0.0)){o.interact_status|=INT_STATUS_INTERACTED;let v=o.damage_or_coin_value.max(0) as i16;self.mario.num_coins=self.mario.num_coins.saturating_add(v);self.mario.heal_counter=self.mario.heal_counter.saturating_add((v as u8).saturating_mul(4));}self.mario.collided_obj_interact_types&=!INTERACT_COIN;}}
 fn finish_object_frame(&mut self){for o in &mut self.objects{if o.active{o.finish_frame();}}}
 fn update_inputs(&mut self,input:Sm64Input){use sm64_core::*;{let m=&mut self.mario;m.particle_flags=0;m.input=0;m.flags&=0x00FF_FFFF;let ap=input.button_a&&!self.previous_input.button_a;let bp=input.button_b&&!self.previous_input.button_b;let zp=input.button_z&&!self.previous_input.button_z;if ap{m.input|=INPUT_A_PRESSED as u16;m.frames_since_a=0}else if m.frames_since_a<0xFF{m.frames_since_a+=1}if input.button_a{m.input|=INPUT_A_DOWN as u16}if m.squish_timer==0{if bp{m.input|=INPUT_B_PRESSED as u16;m.frames_since_b=0}else if m.frames_since_b<0xFF{m.frames_since_b+=1}if input.button_z{m.input|=INPUT_Z_DOWN as u16}if zp{m.input|=INPUT_Z_PRESSED as u16}}let sx=input.stick_x as f32;let sy=input.stick_y as f32;m.stick_x=sx;m.stick_y=sy;let raw=(sx*sx+sy*sy).sqrt().min(64.0);let mag=(raw/64.0)*(raw/64.0)*64.0;m.stick_mag=raw;m.intended_mag=if m.squish_timer==0{mag/2.0}else{mag/8.0};if m.intended_mag>0.0{m.intended_yaw=sm64_core::math::atan2s(-sy,sx).wrapping_add(input.camera_yaw);m.input|=INPUT_NONZERO_ANALOG as u16}else{m.intended_yaw=m.face_angle[1];}}self.refresh_geometry();{let m=&mut self.mario;if m.input&((INPUT_NONZERO_ANALOG|INPUT_A_PRESSED)as u16)==0{m.input|=INPUT_UNKNOWN_5 as u16}if m.wall_kick_timer>0{m.wall_kick_timer-=1}if m.double_jump_timer>0{m.double_jump_timer-=1}}}
 fn refresh_geometry(&mut self){use sm64_core::*;let m=&mut self.mario;self.collision.resolve_walls(&mut m.pos,60.0,50.0);self.collision.resolve_walls(&mut m.pos,30.0,24.0);let floor=self.collision.find_floor(m.pos[0],m.pos[1],m.pos[2]);m.floor=floor.map(|h|h.surface);m.floor_height=floor.map_or(-11000.0,|h|h.height);let ceil=self.collision.find_ceil(m.pos[0],m.floor_height+80.0,m.pos[2]);m.ceil=ceil.map(|h|h.surface);m.ceil_height=ceil.map_or(20000.0,|h|h.height);m.water_level=self.collision.water_level(m.pos[0],m.pos[2])as i16;if let Some(fid)=m.floor{if let Some(s)=self.collision.surface(fid){m.floor_angle=sm64_core::math::atan2s(s.normal.z,s.normal.x);}if m.pos[1]>m.water_level as f32-40.0&&sm64_core::mario_floor_is_slippery(m,&self.collision){m.input|=INPUT_ABOVE_SLIDE as u16}let fd=self.collision.surface(fid).is_some_and(|s|s.flags as i32&SURFACE_FLAG_DYNAMIC!=0);let cd=m.ceil.and_then(|id|self.collision.surface(id)).is_some_and(|s|s.flags as i32&SURFACE_FLAG_DYNAMIC!=0);if(fd||cd)&&(0.0..=150.0).contains(&(m.ceil_height-m.floor_height)){m.input|=INPUT_SQUISHED as u16}if m.pos[1]>m.floor_height+100.0{m.input|=INPUT_OFF_FLOOR as u16}if m.pos[1]<m.water_level as f32-10.0{m.input|=INPUT_IN_WATER as u16}let gas=self.collision.poison_gas_level(m.pos[0],m.pos[2]);if m.pos[1]<gas-100.0{m.input|=INPUT_IN_POISON_GAS as u16}}}
}

fn approach_f32(current:f32,target:f32,step:f32)->f32{if current<target{(current+step).min(target)}else{(current-step).max(target)}}
#[derive(Clone,Copy)]enum CoinFormationKind{LineHorizontal,LineVertical,RingHorizontal,RingVertical,Arrow}
fn coin_formation_kind(e:&str)->CoinFormationKind{if e.contains("LINE_VERTICAL"){CoinFormationKind::LineVertical}else if e.contains("RING_HORIZONTAL"){CoinFormationKind::RingHorizontal}else if e.contains("RING_VERTICAL"){CoinFormationKind::RingVertical}else if e.contains("ARROW"){CoinFormationKind::Arrow}else{CoinFormationKind::LineHorizontal}}
fn coin_formation_offset(k:CoinFormationKind,i:i32)->Option<([f32;3],bool)>{let flying=matches!(k,CoinFormationKind::LineVertical|CoinFormationKind::RingVertical);let r=match k{CoinFormationKind::LineHorizontal=>{if i>4{return None}[0.0,0.0,160.0*(i-2)as f32]},CoinFormationKind::LineVertical=>{if i>4{return None}[0.0,128.0*i as f32,0.0]},CoinFormationKind::RingHorizontal=>{let a=(i as i16)<<13;[sm64_core::math::sins(a)*300.0,0.0,sm64_core::math::coss(a)*300.0]},CoinFormationKind::RingVertical=>{let a=(i as i16)<<13;[sm64_core::math::coss(a)*200.0,sm64_core::math::sins(a)*200.0+200.0,0.0]},CoinFormationKind::Arrow=>{const P:[[f32;2];8]=[[0.0,-150.0],[0.0,-50.0],[0.0,50.0],[0.0,150.0],[-50.0,100.0],[-100.0,50.0],[50.0,100.0],[100.0,50.0]];[P[i as usize][0],0.0,P[i as usize][1]]}};Some((r,!flying))}
fn hitboxes_overlap(a:[f32;3],ar:f32,ah:f32,ad:f32,b:[f32;3],br:f32,bh:f32,bd:f32)->bool{let ab=a[1]-ad;let bb=b[1]-bd;let dx=a[0]-b[0];let dz=a[2]-b[2];if ar+br<=(dx*dx+dz*dz).sqrt(){return false}!(ab>bb+bh||ab+ah<bb)}

#[cfg(test)]mod tests{use super::*;use sm64_core::Surface;#[test]fn simulation_ticks_at_a_stable_integer_boundary(){let mut w=Sm64World::new();assert_eq!(w.step(Sm64Input::default()).tick,1);assert_eq!(w.step(Sm64Input::default()).tick,2);}#[test]fn idle_stick_input_transitions_to_walking(){let mut w=Sm64World::new();w.collision.push_surface(Surface::from_triangle(0,0,0,0,[-1000,0,-1000],[1000,0,-1000],[0,0,1000]).unwrap());w.spawn_mario([0.0,0.0,0.0],0);w.step(Sm64Input{stick_y:-64,..Default::default()});assert_eq!(w.mario.action,sm64_core::ACT_WALKING);}#[test]fn coin_overlap_uses_original_cylinders(){assert!(hitboxes_overlap([0.0,0.0,0.0],37.0,160.0,0.0,[100.0,0.0,0.0],100.0,64.0,0.0));assert!(!hitboxes_overlap([0.0,0.0,0.0],37.0,160.0,0.0,[138.0,0.0,0.0],100.0,64.0,0.0));}}
