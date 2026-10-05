use crate::{
    math::{atan2s, coss, sins, Vec3f, Vec3s},
    types::{ObjectId, SurfaceId},
    CollisionWorld,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum ObjectList {
    Spawner = 0,
    Surface = 1,
    PoleLike = 2,
    Player = 3,
    Pushable = 4,
    GenActor = 5,
    Destructive = 6,
    Level = 7,
    #[default]
    Default = 8,
    Unimportant = 9,
}

impl ObjectList {
    pub const UPDATE_ORDER: [Self; 10] = [
        Self::Spawner,
        Self::Surface,
        Self::PoleLike,
        Self::Player,
        Self::Pushable,
        Self::GenActor,
        Self::Destructive,
        Self::Level,
        Self::Default,
        Self::Unimportant,
    ];

    pub fn from_symbol(symbol: &str) -> Self {
        match symbol.trim() {
            "OBJ_LIST_SPAWNER" => Self::Spawner,
            "OBJ_LIST_SURFACE" => Self::Surface,
            "OBJ_LIST_POLELIKE" => Self::PoleLike,
            "OBJ_LIST_PLAYER" => Self::Player,
            "OBJ_LIST_PUSHABLE" => Self::Pushable,
            "OBJ_LIST_GENACTOR" => Self::GenActor,
            "OBJ_LIST_DESTRUCTIVE" => Self::Destructive,
            "OBJ_LIST_LEVEL" => Self::Level,
            "OBJ_LIST_UNIMPORTANT" => Self::Unimportant,
            _ => Self::Default,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sm64Object {
    pub id: ObjectId,
    pub active: bool,
    pub model: String,
    pub behavior: String,
    pub object_list: ObjectList,

    pub pos: Vec3f,
    pub home: Vec3f,
    pub vel: Vec3f,
    pub forward_vel: f32,
    pub move_angle: Vec3s,
    pub face_angle: Vec3s,
    pub angle_vel: Vec3s,
    pub scale: Vec3f,
    /// Original GraphNodeObject render flags from the native decomp.
    pub render_flags: u16,
    /// Live decomp animation selection/frame. These are presentation state;
    /// gameplay remains authoritative in the native runtime.
    pub anim_id: i16,
    pub anim_frame: i16,

    pub action: i32,
    pub prev_action: i32,
    pub sub_action: i32,
    pub timer: i32,
    pub anim_state: i32,

    pub behavior_params: u32,
    pub behavior_params_expr: String,
    pub behavior_params_2nd_byte: u8,

    pub gravity: f32,
    pub friction: f32,
    pub buoyancy: f32,
    pub bounciness: f32,

    pub floor: Option<SurfaceId>,
    pub floor_height: f32,
    pub floor_type: i16,
    pub move_flags: u32,

    pub distance_to_mario: f32,
    pub angle_to_mario: i16,
    pub angle_to_home: i16,

    pub interact_type: u32,
    pub interact_status: u32,
    pub interaction_subtype: u32,
    pub tangible: bool,
    pub hitbox_radius: f32,
    pub hitbox_height: f32,
    pub hurtbox_radius: f32,
    pub hurtbox_height: f32,
    pub damage_or_coin_value: i32,
    pub health: i32,
    pub num_loot_coins: i32,

    pub parent: Option<ObjectId>,
    pub platform: Option<ObjectId>,
    pub drawing_distance: f32,
    pub opacity: i32,

    /// General-purpose behavior storage mirroring the original object-specific
    /// fields at 0x0F4..0x110. Keeping these indexed lets behavior ports stay
    /// close to the decomp without growing this struct for every enemy.
    pub behavior_i32: [i32; 8],
    pub behavior_f32: [f32; 8],
}

impl Sm64Object {
    pub fn new(
        id: ObjectId,
        model: impl Into<String>,
        behavior: impl Into<String>,
        object_list: ObjectList,
        pos: Vec3f,
        face_angle: Vec3s,
    ) -> Self {
        Self {
            id,
            active: true,
            model: model.into(),
            behavior: behavior.into(),
            object_list,
            pos,
            home: pos,
            vel: [0.0; 3],
            forward_vel: 0.0,
            move_angle: face_angle,
            face_angle,
            angle_vel: [0; 3],
            scale: [1.0; 3],
            render_flags: 1,
            anim_id: -1,
            anim_frame: 0,
            action: 0,
            prev_action: 0,
            sub_action: 0,
            timer: 0,
            anim_state: 0,
            behavior_params: 0,
            behavior_params_expr: String::new(),
            behavior_params_2nd_byte: 0,
            gravity: 0.0,
            friction: 1.0,
            buoyancy: 0.0,
            bounciness: 0.0,
            floor: None,
            floor_height: -11000.0,
            floor_type: 0,
            move_flags: 0,
            distance_to_mario: 0.0,
            angle_to_mario: 0,
            angle_to_home: 0,
            interact_type: 0,
            interact_status: 0,
            interaction_subtype: 0,
            tangible: true,
            hitbox_radius: 0.0,
            hitbox_height: 0.0,
            hurtbox_radius: 0.0,
            hurtbox_height: 0.0,
            damage_or_coin_value: 0,
            health: 0,
            num_loot_coins: 0,
            parent: None,
            platform: None,
            drawing_distance: 4000.0,
            opacity: 255,
            behavior_i32: [0; 8],
            behavior_f32: [0.0; 8],
        }
    }

    pub fn set_action(&mut self, action: i32) {
        if self.action != action {
            self.prev_action = self.action;
            self.action = action;
            self.timer = 0;
            self.sub_action = 0;
        }
    }

    pub fn begin_frame(&mut self, mario_pos: Vec3f) {
        let dx = mario_pos[0] - self.pos[0];
        let dy = mario_pos[1] - self.pos[1];
        let dz = mario_pos[2] - self.pos[2];
        self.distance_to_mario = (dx * dx + dy * dy + dz * dz).sqrt();
        self.angle_to_mario = atan2s(dz, dx);
        self.angle_to_home = atan2s(self.home[2] - self.pos[2], self.home[0] - self.pos[0]);
    }

    pub fn finish_frame(&mut self) {
        if self.action == self.prev_action {
            self.timer = self.timer.saturating_add(1);
        } else {
            self.prev_action = self.action;
            self.timer = 0;
        }
    }

    pub fn set_forward_velocity(&mut self, speed: f32) {
        self.forward_vel = speed;
        self.vel[0] = sins(self.move_angle[1]) * speed;
        self.vel[2] = coss(self.move_angle[1]) * speed;
    }

    pub fn refresh_floor(&mut self, collision: &CollisionWorld) {
        if let Some(hit) = collision.find_floor(self.pos[0], self.pos[1] + 100.0, self.pos[2]) {
            self.floor = Some(hit.surface);
            self.floor_height = hit.height;
            self.floor_type = collision
                .surface(hit.surface)
                .map_or(0, |surface| surface.surface_type);
        } else {
            self.floor = None;
            self.floor_height = -11000.0;
            self.floor_type = 0;
        }
    }
}
