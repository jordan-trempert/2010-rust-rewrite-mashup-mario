use sm64_core::{CollisionWorld, MarioState};

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
}

#[derive(Clone, Debug, Default)]
pub struct Sm64World {
    pub tick: u64,
    pub mario: MarioState,
    pub collision: CollisionWorld,
    previous_input: Sm64Input,
}

impl Sm64World {
    pub fn new() -> Self { Self::default() }

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
            sm64_core::mario_action::set_mario_action(&mut self.mario,&self.collision,ACT_IDLE,0);
        }
    }

    pub fn snapshot(&self) -> Sm64Snapshot {
        Sm64Snapshot { tick: self.tick, mario: self.mario.clone() }
    }

    pub fn step(&mut self, input: Sm64Input) -> Sm64Snapshot {
        self.tick=self.tick.wrapping_add(1);
        self.update_inputs(input);
        if self.mario.floor.is_some() {
            sm64_core::actions::execute_mario_action(&mut self.mario,&self.collision);
        }
        self.previous_input=input;
        self.snapshot()
    }

    fn update_inputs(&mut self, input:Sm64Input) {
        use sm64_core::*;
        {
            let m=&mut self.mario;
            m.particle_flags=0;
            m.input=0;
            // Original code clears transient high flag bits on every frame.
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
            let raw_mag=(sx*sx+sy*sy).sqrt().min(64.0);
            let mag=(raw_mag/64.0)*(raw_mag/64.0)*64.0;
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
}
