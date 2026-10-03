use sm64_core::{
    CollisionWorld, MarioState, ObjectId, ObjectList, Sm64Object,
    ACT_FLAG_SHORT_HITBOX, INTERACT_COIN, INT_STATUS_INTERACTED,
};

pub const SM64_TICK_HZ: u32 = 30;
pub const SM64_TICK_SECONDS: f64 = 1.0 / SM64_TICK_HZ as f64;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sm64Input {
    pub stick_x: i8,
    pub stick_y: i8,
    pub camera_yaw: i16,
    pub button_a: bool,
    pub button_b: bool,
    pub button_z: bool,
    pub button_start: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sm64Snapshot {
    pub tick: u64,
    pub mario: MarioState,
    pub objects: Vec<Sm64Object>,
}

#[derive(Clone, Debug)]
pub struct Sm64World {
    pub tick: u64,
    pub mario: MarioState,
    pub collision: CollisionWorld,
    pub objects: Vec<Sm64Object>,
    previous_input: Sm64Input,
    next_object_id: u32,
}

impl Default for Sm64World {
    fn default() -> Self {
        Self {
            tick: 0,
            mario: MarioState::default(),
            collision: CollisionWorld::default(),
            objects: Vec::new(),
            previous_input: Sm64Input::default(),
            next_object_id: 1,
        }
    }
}

impl Sm64World {
    pub fn new() -> Self { Self::default() }

    pub fn clear_objects(&mut self) {
        self.objects.clear();
        self.next_object_id = 1;
    }

    pub fn spawn_object(
        &mut self,
        model: impl Into<String>,
        behavior: impl Into<String>,
        object_list: ObjectList,
        pos: [f32;3],
        face_angle: [i16;3],
    ) -> ObjectId {
        let id=ObjectId(self.next_object_id);
        self.next_object_id=self.next_object_id.wrapping_add(1).max(1);
        self.objects.push(Sm64Object::new(
            id,model,behavior,object_list,pos,face_angle,
        ));
        id
    }

    pub fn object(&self,id:ObjectId)->Option<&Sm64Object>{
        self.objects.iter().find(|object|object.id==id)
    }

    pub fn object_mut(&mut self,id:ObjectId)->Option<&mut Sm64Object>{
        self.objects.iter_mut().find(|object|object.id==id)
    }

    pub fn spawn_mario(&mut self, pos:[f32;3], yaw:i16) {
        use sm64_core::*;
        self.mario=MarioState::default();
        self.mario.pos=pos;
        self.mario.face_angle=[0,yaw,0];
        self.mario.flags=MARIO_NORMAL_CAP | MARIO_CAP_ON_HEAD;
        self.mario.frames_since_a=0xFF;
        self.mario.frames_since_b=0xFF;
        self.refresh_geometry();
        if let Some(floor)=self.mario.floor {
            let _=floor;
            if self.mario.pos[1] < self.mario.floor_height { self.mario.pos[1]=self.mario.floor_height; }
            let initial=if self.mario.pos[1] <= self.mario.water_level as f32-100.0 {
                ACT_WATER_IDLE
            } else {
                ACT_IDLE
            };
            sm64_core::mario_action::set_mario_action(&mut self.mario,&self.collision,initial,0);
        }
    }

    pub fn snapshot(&self) -> Sm64Snapshot {
        Sm64Snapshot {
            tick:self.tick,
            mario:self.mario.clone(),
            objects:self.objects.iter().filter(|o|o.active).cloned().collect(),
        }
    }

    pub fn step(&mut self, input: Sm64Input) -> Sm64Snapshot {
        self.tick=self.tick.wrapping_add(1);
        self.mario.global_timer=self.tick as u32;

        // The original frame updates terrain/surface objects before Mario/object
        // collision, then processes the remaining lists in a fixed order.
        self.update_object_metrics();
        self.update_object_lists_before_player();

        self.update_inputs(input);
        self.detect_mario_object_collisions();
        self.process_mario_object_interactions();

        if self.mario.floor.is_some() {
            sm64_core::actions::execute_mario_action(&mut self.mario,&self.collision);
        }

        self.update_object_lists_after_player();
        self.finish_object_frame();
        self.objects.retain(|object|object.active);
        self.previous_input=input;
        self.snapshot()
    }

    fn update_object_metrics(&mut self) {
        let mario_pos=self.mario.pos;
        for object in &mut self.objects {
            if object.active {
                object.begin_frame(mario_pos);
                object.refresh_floor(&self.collision);
            }
        }
    }

    fn update_object_lists_before_player(&mut self) {
        for list in [ObjectList::Spawner,ObjectList::Surface,ObjectList::PoleLike] {
            self.update_object_list(list);
        }
    }

    fn update_object_lists_after_player(&mut self) {
        for list in [
            ObjectList::Pushable,
            ObjectList::GenActor,
            ObjectList::Destructive,
            ObjectList::Level,
            ObjectList::Default,
            ObjectList::Unimportant,
        ] {
            self.update_object_list(list);
        }
    }

    fn update_object_list(&mut self,list:ObjectList) {
        let indices=self.objects.iter().enumerate()
            .filter_map(|(index,object)|(object.active&&object.object_list==list).then_some(index))
            .collect::<Vec<_>>();
        for index in indices {
            self.update_object_behavior(index);
        }
    }

    fn update_object_behavior(&mut self,index:usize) {
        let behavior=self.objects[index].behavior.clone();
        match behavior.as_str() {
            "bhvYellowCoin"|"bhvYellowCoinInit"|"bhvCoinFormationSpawn" => {
                self.update_yellow_coin(index);
            }
            _ => {}
        }
    }

    fn update_yellow_coin(&mut self,index:usize) {
        let object=&mut self.objects[index];
        if object.timer==0 {
            object.interact_type=INTERACT_COIN;
            object.damage_or_coin_value=1;
            object.hitbox_radius=100.0;
            object.hitbox_height=64.0;
            object.anim_state=0;
        }
        object.anim_state=object.anim_state.wrapping_add(1);

        if object.interact_status & INT_STATUS_INTERACTED != 0 {
            object.active=false;
        } else {
            object.interact_status=0;
        }
    }

    fn detect_mario_object_collisions(&mut self) {
        self.mario.collided_obj_interact_types=0;
        let mario_radius=37.0;
        let mario_height=if self.mario.action & ACT_FLAG_SHORT_HITBOX != 0 {100.0}else{160.0};

        for object in &mut self.objects {
            if !object.active || !object.tangible || object.hitbox_radius<=0.0 || object.hitbox_height<=0.0 {
                continue;
            }
            if hitboxes_overlap(
                self.mario.pos,mario_radius,mario_height,0.0,
                object.pos,object.hitbox_radius,object.hitbox_height,0.0,
            ) {
                self.mario.collided_obj_interact_types|=object.interact_type;
                // Interaction priority is handled separately. Mark only after
                // the Mario-side handler actually consumes the collision.
            }
        }
    }

    fn process_mario_object_interactions(&mut self) {
        // Interaction.c processes coins first. Preserve that ordering.
        if self.mario.collided_obj_interact_types & INTERACT_COIN != 0 {
            let mario_pos=self.mario.pos;
            let mario_radius=37.0;
            let mario_height=if self.mario.action & ACT_FLAG_SHORT_HITBOX != 0 {100.0}else{160.0};

            if let Some(object)=self.objects.iter_mut().find(|object|{
                object.active
                    && object.interact_type & INTERACT_COIN != 0
                    && hitboxes_overlap(
                        mario_pos,mario_radius,mario_height,0.0,
                        object.pos,object.hitbox_radius,object.hitbox_height,0.0,
                    )
            }) {
                object.interact_status|=INT_STATUS_INTERACTED;
                let value=object.damage_or_coin_value.max(0) as i16;
                self.mario.num_coins=self.mario.num_coins.saturating_add(value);
                // SM64 coins heal 4 wedges per coin value via healCounter.
                self.mario.heal_counter=self.mario.heal_counter.saturating_add((value as u8).saturating_mul(4));
            }
            self.mario.collided_obj_interact_types&=!INTERACT_COIN;
        }
    }

    fn finish_object_frame(&mut self) {
        for object in &mut self.objects {
            if object.active {
                object.finish_frame();
            }
        }
    }

    fn update_inputs(&mut self, input:Sm64Input) {
        use sm64_core::*;
        {
            let m=&mut self.mario;
            m.particle_flags=0;
            m.input=0;
            m.flags &= 0x00FF_FFFF;

            let a_pressed=input.button_a && !self.previous_input.button_a;
            let b_pressed=input.button_b && !self.previous_input.button_b;
            let z_pressed=input.button_z && !self.previous_input.button_z;

            if a_pressed { m.input|=INPUT_A_PRESSED as u16; m.frames_since_a=0; }
            else if m.frames_since_a<0xFF {m.frames_since_a+=1;}
            if input.button_a {m.input|=INPUT_A_DOWN as u16;}

            if m.squish_timer==0 {
                if b_pressed {m.input|=INPUT_B_PRESSED as u16; m.frames_since_b=0;}
                else if m.frames_since_b<0xFF {m.frames_since_b+=1;}
                if input.button_z {m.input|=INPUT_Z_DOWN as u16;}
                if z_pressed {m.input|=INPUT_Z_PRESSED as u16;}
            }

            let sx=input.stick_x as f32;
            let sy=input.stick_y as f32;
            m.stick_x=sx;
            m.stick_y=sy;
            let raw_mag=(sx*sx+sy*sy).sqrt().min(64.0);
            let mag=(raw_mag/64.0)*(raw_mag/64.0)*64.0;
            m.stick_mag=raw_mag;
            m.intended_mag=if m.squish_timer==0 {mag/2.0}else{mag/8.0};
            if m.intended_mag>0.0 {
                m.intended_yaw=sm64_core::math::atan2s(-sy,sx).wrapping_add(input.camera_yaw);
                m.input|=INPUT_NONZERO_ANALOG as u16;
            } else {
                m.intended_yaw=m.face_angle[1];
            }
        }

        self.refresh_geometry();

        {
            let m=&mut self.mario;
            if m.input & ((INPUT_NONZERO_ANALOG|INPUT_A_PRESSED) as u16)==0 {
                m.input|=INPUT_UNKNOWN_5 as u16;
            }
            if m.wall_kick_timer>0 {m.wall_kick_timer-=1;}
            if m.double_jump_timer>0 {m.double_jump_timer-=1;}
        }
    }

    fn refresh_geometry(&mut self) {
        use sm64_core::*;
        let m=&mut self.mario;
        self.collision.resolve_walls(&mut m.pos,60.0,50.0);
        self.collision.resolve_walls(&mut m.pos,30.0,24.0);
        let floor=self.collision.find_floor(m.pos[0],m.pos[1],m.pos[2]);
        m.floor=floor.map(|h|h.surface);
        m.floor_height=floor.map_or(-11000.0,|h|h.height);
        let ceil=self.collision.find_ceil(m.pos[0],m.floor_height+80.0,m.pos[2]);
        m.ceil=ceil.map(|h|h.surface);
        m.ceil_height=ceil.map_or(20000.0,|h|h.height);
        m.water_level=self.collision.water_level(m.pos[0],m.pos[2]) as i16;
        if let Some(floor_id)=m.floor {
            if let Some(s)=self.collision.surface(floor_id) {
                m.floor_angle=sm64_core::math::atan2s(s.normal.z,s.normal.x);
            }

            if m.pos[1] > m.water_level as f32-40.0
                && sm64_core::mario_floor_is_slippery(m,&self.collision)
            {
                m.input|=INPUT_ABOVE_SLIDE as u16;
            }

            let floor_dynamic=self.collision.surface(floor_id)
                .is_some_and(|s|s.flags as i32 & SURFACE_FLAG_DYNAMIC != 0);
            let ceil_dynamic=m.ceil
                .and_then(|id|self.collision.surface(id))
                .is_some_and(|s|s.flags as i32 & SURFACE_FLAG_DYNAMIC != 0);
            let ceil_floor_dist=m.ceil_height-m.floor_height;
            if (floor_dynamic||ceil_dynamic) && (0.0..=150.0).contains(&ceil_floor_dist) {
                m.input|=INPUT_SQUISHED as u16;
            }

            if m.pos[1] > m.floor_height+100.0 {m.input|=INPUT_OFF_FLOOR as u16;}
            if m.pos[1] < m.water_level as f32-10.0 {m.input|=INPUT_IN_WATER as u16;}
            let gas=self.collision.poison_gas_level(m.pos[0],m.pos[2]);
            if m.pos[1] < gas-100.0 {m.input|=INPUT_IN_POISON_GAS as u16;}
        }
    }
}

fn hitboxes_overlap(
    a_pos:[f32;3],a_radius:f32,a_height:f32,a_down:f32,
    b_pos:[f32;3],b_radius:f32,b_height:f32,b_down:f32,
)->bool{
    let a_bottom=a_pos[1]-a_down;
    let b_bottom=b_pos[1]-b_down;
    let dx=a_pos[0]-b_pos[0];
    let dz=a_pos[2]-b_pos[2];
    let radius=a_radius+b_radius;

    if radius <= (dx*dx+dz*dz).sqrt() {return false;}
    let a_top=a_bottom+a_height;
    let b_top=b_bottom+b_height;
    !(a_bottom>b_top || a_top<b_bottom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm64_core::Surface;

    #[test]
    fn simulation_ticks_at_a_stable_integer_boundary() {
        let mut w=Sm64World::new();
        assert_eq!(w.step(Sm64Input::default()).tick,1);
        assert_eq!(w.step(Sm64Input::default()).tick,2);
    }

    #[test]
    fn idle_stick_input_transitions_to_walking() {
        let mut w=Sm64World::new();
        w.collision.push_surface(Surface::from_triangle(0,0,0,0,[-1000,0,-1000],[1000,0,-1000],[0,0,1000]).unwrap());
        w.spawn_mario([0.0,0.0,0.0],0);
        w.step(Sm64Input{stick_y:-64,..Default::default()});
        assert_eq!(w.mario.action,sm64_core::ACT_WALKING);
    }

    #[test]
    fn coin_overlap_uses_original_cylinders() {
        assert!(hitboxes_overlap(
            [0.0,0.0,0.0],37.0,160.0,0.0,
            [100.0,0.0,0.0],100.0,64.0,0.0,
        ));
        assert!(!hitboxes_overlap(
            [0.0,0.0,0.0],37.0,160.0,0.0,
            [138.0,0.0,0.0],100.0,64.0,0.0,
        ));
    }
}
