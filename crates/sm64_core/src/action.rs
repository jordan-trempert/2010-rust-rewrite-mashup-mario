pub const ACT_ID_MASK: u32 = 0x0000_01ff;

pub const ACT_GROUP_MASK: u32 = 0x0000_01c0;
pub const ACT_GROUP_STATIONARY: u32 = 0 << 6;
pub const ACT_GROUP_MOVING: u32 = 1 << 6;
pub const ACT_GROUP_AIRBORNE: u32 = 2 << 6;
pub const ACT_GROUP_SUBMERGED: u32 = 3 << 6;
pub const ACT_GROUP_CUTSCENE: u32 = 4 << 6;
pub const ACT_GROUP_AUTOMATIC: u32 = 5 << 6;
pub const ACT_GROUP_OBJECT: u32 = 6 << 6;

pub const ACT_FLAG_STATIONARY: u32 = 1 << 9;
pub const ACT_FLAG_MOVING: u32 = 1 << 10;
pub const ACT_FLAG_AIR: u32 = 1 << 11;
pub const ACT_FLAG_INTANGIBLE: u32 = 1 << 12;
pub const ACT_FLAG_SWIMMING: u32 = 1 << 13;
pub const ACT_FLAG_METAL_WATER: u32 = 1 << 14;
pub const ACT_FLAG_SHORT_HITBOX: u32 = 1 << 15;
pub const ACT_FLAG_RIDING_SHELL: u32 = 1 << 16;
pub const ACT_FLAG_INVULNERABLE: u32 = 1 << 17;
pub const ACT_FLAG_BUTT_OR_STOMACH_SLIDE: u32 = 1 << 18;
pub const ACT_FLAG_DIVING: u32 = 1 << 19;
pub const ACT_FLAG_ON_POLE: u32 = 1 << 20;
pub const ACT_FLAG_HANGING: u32 = 1 << 21;
pub const ACT_FLAG_IDLE: u32 = 1 << 22;
pub const ACT_FLAG_ATTACKING: u32 = 1 << 23;
pub const ACT_FLAG_ALLOW_VERTICAL_WIND_ACTION: u32 = 1 << 24;
pub const ACT_FLAG_CONTROL_JUMP_HEIGHT: u32 = 1 << 25;
pub const ACT_FLAG_ALLOW_FIRST_PERSON: u32 = 1 << 26;
pub const ACT_FLAG_PAUSE_EXIT: u32 = 1 << 27;
pub const ACT_FLAG_SWIMMING_OR_FLYING: u32 = 1 << 28;
pub const ACT_FLAG_WATER_OR_TEXT: u32 = 1 << 29;
pub const ACT_FLAG_THROWING: u32 = 1 << 31;

#[inline]
pub const fn action_group(action: u32) -> u32 {
    action & ACT_GROUP_MASK
}
