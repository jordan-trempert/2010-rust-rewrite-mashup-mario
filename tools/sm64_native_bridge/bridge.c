#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#include <windows.h>
#endif

#include "sm64.h"
#include "types.h"
#include "object_fields.h"
#include "object_constants.h"
#include "model_ids.h"
#include "level_commands.h"
#include "dialog_ids.h"
#include "levels/scripts.h"
#include "game/area.h"
#include "game/game_init.h"
#include "game/interaction.h"
#include "game/ingame_menu.h"
#include "game/segment2.h"
#include "game/level_update.h"
#include "game/mario.h"
#include "game/memory.h"
#include "game/object_list_processor.h"
#include "game/platform_displacement.h"
#include "game/save_file.h"
#include "audio/external.h"
#include "audio/audio_api.h"
#include "audio/audio_null.h"
#include "audio/audio_wasapi.h"
#include "gfx/gfx_pc.h"
#include "gfx/gfx_dummy.h"
#include "engine/level_script.h"
#include "engine/math_util.h"
#include "engine/surface_collision.h"
#include "engine/surface_load.h"

#define REQUEST_MAGIC 0x31514D53u /* SMQ1 */
#define SNAPSHOT_MAGIC 0x31534D53u /* SMS1 */
#define OP_STEP 1u
#define OP_SHUTDOWN 2u
#define BRIDGE_INPUT_ATTACK 0x00000001u
#define BRIDGE_INPUT_USE    0x00000002u

#if defined(_WIN32) || defined(_WIN64)
#define IW4L_SM64_API __declspec(dllexport)
#else
#define IW4L_SM64_API __attribute__((visibility("default")))
#endif

#define IW4L_SM64_ABI_VERSION 3u
static volatile const char *gBridgeStage = "startup";
static int gBridgeDialogPendingReset = 0;

/* ingame_menu.c owns these. The normal renderer advances them, but the bridge
   intentionally removes render_game(), so the headless host must complete the
   close/response edge itself. */
extern s16 gDialogID;
extern s32 gDialogResponse;
extern s8 gLastDialogResponse;


OSMesg gMainReceivedMesg;
OSMesgQueue gSIEventMesgQueue;

s8 gResetTimer;
s8 gNmiResetBarsTimer;
s8 gDebugLevelSelect;
s8 gShowProfiler;
s8 gShowDebugText;

extern void thread5_game_loop(void *arg);

extern void create_next_audio_buffer(s16 *samples, u32 num_samples);

#ifdef VERSION_EU
#define IW4L_AUDIO_SAMPLES_HIGH 656
#define IW4L_AUDIO_SAMPLES_LOW 640
#else
#define IW4L_AUDIO_SAMPLES_HIGH 544
#define IW4L_AUDIO_SAMPLES_LOW 528
#endif

/*
 * Never open a second Windows audio device from the embedded SM64 DLL.
 * COD/Bevy owns the process audio device. Running sm64-port's WASAPI backend
 * beside Bevy/rodio let a later COD voice steal/reset the device and made the
 * SM64 stream go permanently silent.
 *
 * Instead, generate the original 32 kHz stereo PCM for this native tick and
 * expose it in the snapshot. Rust queues it into the same match audio bus as
 * COD, so both games are mixed by one device owner.
 */
#define IW4L_AUDIO_SAMPLE_RATE 32000u
#define IW4L_AUDIO_CHANNELS 2u
#define IW4L_AUDIO_MAX_FRAMES 1088u

static s16 gIw4lAudioSamples[IW4L_AUDIO_MAX_FRAMES * IW4L_AUDIO_CHANNELS];
static uint32_t gIw4lAudioFrameCount = 0;
static uint32_t gIw4lAudioCadence = 0;

static void bridge_audio_backend_init(void) {
    gIw4lAudioFrameCount = 0;
    gIw4lAudioCadence = 0;
    memset(gIw4lAudioSamples, 0, sizeof(gIw4lAudioSamples));
}

static void bridge_audio_tick(void) {
    uint32_t frames_per_buffer;
    int i;

#ifdef VERSION_EU
    /*
     * PAL's 640-sample low buffer already yields 1280 frames per 25 Hz game
     * tick. The embedded gameplay path is normally built as US, but keep the
     * native PAL buffer contract intact if that changes.
     */
    frames_per_buffer = IW4L_AUDIO_SAMPLES_LOW;
#else
    /*
     * The original US synthesizer expects each task to be either 528 or 544
     * frames (16-frame aligned). Two LOW tasks are 1056 frames and two HIGH
     * tasks are 1088. Using HIGH once every three native 30 Hz ticks averages
     * exactly 1066 2/3 frames/tick = 32000 Hz, without ever feeding an
     * unsupported odd-sized synthesis task.
     */
    gIw4lAudioCadence = (gIw4lAudioCadence + 1u) % 3u;
    frames_per_buffer =
        gIw4lAudioCadence == 0u ? IW4L_AUDIO_SAMPLES_HIGH : IW4L_AUDIO_SAMPLES_LOW;
#endif

    for (i = 0; i < 2; ++i) {
        create_next_audio_buffer(
            gIw4lAudioSamples + i * (frames_per_buffer * IW4L_AUDIO_CHANNELS),
            frames_per_buffer
        );
    }
    gIw4lAudioFrameCount = frames_per_buffer * 2u;
}

void dispatch_audio_sptask(UNUSED struct SPTask *spTask) {
}

void set_vblank_handler(
    UNUSED s32 index,
    UNUSED struct VblankHandler *handler,
    UNUSED OSMesgQueue *queue,
    UNUSED OSMesg *msg
) {
}

void exec_display_list(UNUSED struct SPTask *spTask) {
    /* Headless bridge: the host renders every SM64 object itself. */
}

#define DEFINE_LEVEL(internal, level_enum, course_enum, folder, texture, acoustic, echo1, echo2, echo3, dyn, cam) \
    extern const LevelScript level_##folder##_entry[];
#define STUB_LEVEL(internal, level_enum, course_enum, acoustic, echo1, echo2, echo3, dyn, cam)
#include "levels/level_defines.h"
#undef DEFINE_LEVEL
#undef STUB_LEVEL

/*
 * Vanilla never enters a course script directly. level_main_scripts_entry first
 * loads the shared Mario/common model graph nodes, then dispatches to the chosen
 * level through its normal EXECUTE path. Entering level_bob_entry directly can
 * create valid gameplay objects whose sharedChild graph nodes are NULL; the
 * render pass at the end of level_script_execute() then crashes.
 *
 * Give every course a tiny cold-start script that establishes a clean level
 * state, seeds the script register with the requested level, and then jumps into
 * the vanilla shared-level bootstrap. gDebugLevelSelect is enabled only for the
 * first execute so level_main_menu_entry_2 skips the act-select screen.
 */
#define DEFINE_LEVEL(internal, level_enum, course_enum, folder, texture, acoustic, echo1, echo2, echo3, dyn, cam) \
    static const LevelScript bridge_bootstrap_##folder[] = { \
        INIT_LEVEL(), \
        CLEAR_LEVEL(), \
        SET_REG(level_enum), \
        JUMP(level_main_scripts_entry), \
    };
#define STUB_LEVEL(internal, level_enum, course_enum, acoustic, echo1, echo2, echo3, dyn, cam)
#include "levels/level_defines.h"
#undef DEFINE_LEVEL
#undef STUB_LEVEL

struct LevelSpec {
    const char *name;
    s16 level;
    s16 course;
    const LevelScript *entry;
};

static const struct LevelSpec kLevels[] = {
#define DEFINE_LEVEL(internal, level_enum, course_enum, folder, texture, acoustic, echo1, echo2, echo3, dyn, cam) \
    { #folder, level_enum, course_enum, bridge_bootstrap_##folder },
#define STUB_LEVEL(internal, level_enum, course_enum, acoustic, echo1, echo2, echo3, dyn, cam)
#include "levels/level_defines.h"
#undef DEFINE_LEVEL
#undef STUB_LEVEL
};

struct Request {
    uint32_t op;
    float pos[3];
    float vel[3];
    int16_t yaw;
    int16_t pitch;
    int32_t health;
    uint32_t attack_flags;
};

struct Iw4lSm64PlayerProxy {
    float pos[3];
    float vel[3];
    int16_t yaw;
    int16_t pitch;
    int32_t health;
    uint32_t attack_flags;
};

struct Iw4lSm64Object {
    uint32_t id;
    int32_t model_id;
    float pos[3];
    int16_t face_angle[3];
    float scale[3];
    uint16_t active_flags;
    uint16_t render_flags;
    int16_t anim_id;
    int16_t anim_frame;
    int32_t anim_state;
    uint32_t interact_status;
    int32_t damage_or_coin_value;
};

struct Iw4lSm64Triangle {
    float vertices[9];
};

struct Iw4lSm64RenderTriangle {
    float pos[9];
    float uv[6];
    uint8_t rgba[12];
    uint32_t texture_id;
    uint8_t textured;
    uint8_t alpha;
    uint8_t wrap_s;
    uint8_t wrap_t;
};

struct Iw4lSm64TextureView {
    uint32_t id;
    uint32_t width;
    uint32_t height;
    uint32_t generation;
    const uint8_t *rgba;
};

struct Iw4lSm64SnapshotView {
    uint32_t abi_version;
    uint32_t tick;
    int32_t mario_health;
    int32_t coins;
    uint32_t mario_action;
    float mario_pos[3];
    float mario_vel[3];
    int16_t mario_yaw;
    int16_t dialog_id;
    const char *dialog_text;
    uint32_t dialog_text_len;
    const struct Iw4lSm64Object *objects;
    uint32_t object_count;
    const struct Iw4lSm64Triangle *dynamic_surfaces;
    uint32_t dynamic_surface_count;
    const struct Iw4lSm64RenderTriangle *render_triangles;
    uint32_t render_triangle_count;
    const struct Iw4lSm64TextureView *textures;
    uint32_t texture_count;
    const int16_t *audio_samples;
    uint32_t audio_frame_count;
    uint32_t audio_sample_rate;
    uint16_t audio_channels;
    uint16_t audio_reserved;
};

static struct LevelCommand *gIw4lLevelCommand = NULL;
/* Defined in mario.c but not exported by upstream mario.h. */
extern void mario_update_hitbox_and_cap_model(struct MarioState *m);

static int gIw4lInitialized = 0;
/* Read by the temporarily patched bhv_mario_update() in the embedded build.
 * Normal COD locomotion keeps this false; native mechanics set it true. */
int gIw4lRunMarioAction = 0;
static char gIw4lLastError[512];
static char gIw4lDialogText[4096];
static struct Iw4lSm64Object gIw4lObjects[4096];
static struct Iw4lSm64Triangle gIw4lDynamicSurfaces[8192];
static struct Iw4lSm64SnapshotView gIw4lSnapshot;

#define IW4L_SM64_MAX_RENDER_TRIANGLES 65536u
#define IW4L_SM64_MAX_TEXTURES 512u

struct Iw4lSm64TextureStorage {
    struct Iw4lSm64TextureView view;
    uint8_t *owned_rgba;
    size_t capacity;
};

static struct Iw4lSm64RenderTriangle gIw4lRenderTriangles[IW4L_SM64_MAX_RENDER_TRIANGLES];
static uint32_t gIw4lRenderTriangleCount = 0;
static struct Iw4lSm64TextureStorage gIw4lTextureStorage[IW4L_SM64_MAX_TEXTURES];
static struct Iw4lSm64TextureView gIw4lTextureViews[IW4L_SM64_MAX_TEXTURES];
static uint32_t gIw4lTextureCount = 0;

/* Shared with temporary embedded-render patches in rendering_graph_node.c and
 * gfx_pc.c. The native graph traversal uses the COD camera for billboards and
 * culling, then gfx_pc removes this view transform again before exporting
 * world-space vertices. */
int gIw4lUseExternalCamera = 0;
float gIw4lRenderCameraPos[3] = {0.0f,0.0f,0.0f};
float gIw4lRenderCameraFocus[3] = {0.0f,0.0f,-1000.0f};
float gIw4lNativeCameraMatrix[4][4] = {
    {1,0,0,0},{0,1,0,0},{0,0,1,0},{0,0,0,1}
};

static uint32_t object_id(const struct Object *object) {
    /*
     * The native SM64 object pool is fixed-size and object addresses are reused.
     * Use the object's address relative to the pool when possible rather than
     * retaining raw pointers forever in a side table. The low bits remain
     * deterministic for a pool slot and are sufficient for presentation IDs.
     */
    uintptr_t value=(uintptr_t)object;
    return (uint32_t)((value >> 4) ^ (value >> 20));
}

static int32_t model_id_for_object(const struct Object *object) {
    int32_t i;
    struct GraphNode *shared = object->header.gfx.sharedChild;
    if (shared == NULL || gLoadedGraphNodes == NULL) {
        return -1;
    }
    for (i = 0; i < 0x100; ++i) {
        if (gLoadedGraphNodes[i] == shared) {
            return i;
        }
    }
    return -1;
}

static const struct LevelSpec *find_level(const char *name) {
    size_t i;
    for (i = 0; i < sizeof(kLevels) / sizeof(kLevels[0]); ++i) {
        if (strcmp(kLevels[i].name, name) == 0) {
            return &kLevels[i];
        }
    }
    return NULL;
}

static int read_exact(void *dst, size_t size) {
    return fread(dst, 1, size, stdin) == size;
}

static int read_u32(uint32_t *value) { return read_exact(value, sizeof(*value)); }
static int read_i32(int32_t *value) { return read_exact(value, sizeof(*value)); }
static int read_u16(uint16_t *value) { return read_exact(value, sizeof(*value)); }
static int read_i16(int16_t *value) { return read_exact(value, sizeof(*value)); }
static int read_f32(float *value) { return read_exact(value, sizeof(*value)); }

static void write_u32(uint32_t value) { fwrite(&value, sizeof(value), 1, stdout); }
static void write_i32(int32_t value) { fwrite(&value, sizeof(value), 1, stdout); }
static void write_u16(uint16_t value) { fwrite(&value, sizeof(value), 1, stdout); }
static void write_i16(int16_t value) { fwrite(&value, sizeof(value), 1, stdout); }
static void write_f32(float value) { fwrite(&value, sizeof(value), 1, stdout); }

static int read_request(struct Request *request) {
    uint32_t magic;
    int i;
    if (!read_u32(&magic) || magic != REQUEST_MAGIC || !read_u32(&request->op)) {
        return 0;
    }
    for (i = 0; i < 3; ++i) if (!read_f32(&request->pos[i])) return 0;
    for (i = 0; i < 3; ++i) if (!read_f32(&request->vel[i])) return 0;
    if (!read_i16(&request->yaw) || !read_i16(&request->pitch)) return 0;
    if (!read_i32(&request->health) || !read_u32(&request->attack_flags)) return 0;
    return 1;
}

static void bridge_update_headless_dialog(const struct Request *request) {
    if (gBridgeDialogPendingReset && get_dialog_id() == DIALOG_NONE) {
        /*
         * The previous native update had one full tick to consume
         * gDialogResponse / the DIALOG_NONE edge. Reset the visual state now
         * so another dialog can be opened normally.
         */
        reset_dialog_render_state();
        gBridgeDialogPendingReset = 0;
    }

    if ((request->attack_flags & (BRIDGE_INPUT_USE | BRIDGE_INPUT_ATTACK)) != 0 &&
        get_dialog_id() != DIALOG_NONE) {
        /*
         * render_dialog_entries() normally consumes A/B, performs the close
         * animation and publishes the response. render_game() is disabled in
         * this headless bridge, so either COD Use or Fire must emit the same
         * gameplay-visible result directly.
         *
         * create_dialog_box_with_response() seeds gLastDialogResponse to 1;
         * ordinary dialogs leave it at zero and use NOT_DEFINED.
         */
        gDialogResponse = gLastDialogResponse != 0
            ? gLastDialogResponse
            : DIALOG_RESPONSE_NOT_DEFINED;
        gDialogID = DIALOG_NONE;
        gBridgeDialogPendingReset = 1;
    }
}

static int native_owns_mario_motion(void) {
    u32 action;
    u32 group;
    if (gMarioState == NULL) {
        return 0;
    }
    action = gMarioState->action;
    group = action & ACT_GROUP_MASK;

    if (group == ACT_GROUP_OBJECT ||
        group == ACT_GROUP_AUTOMATIC ||
        group == ACT_GROUP_CUTSCENE) {
        return 1;
    }

    switch (action) {
        case ACT_SHOT_FROM_CANNON:
        case ACT_TORNADO_TWIRLING:
        case ACT_GRABBED:
        case ACT_RIDING_HOOT:
        case ACT_WARP_DOOR_SPAWN:
        case ACT_EMERGE_FROM_PIPE:
        case ACT_SPAWN_SPIN_AIRBORNE:
        case ACT_SPAWN_NO_SPIN_AIRBORNE:
        case ACT_TELEPORT_FADE_OUT:
        case ACT_TELEPORT_FADE_IN:
        case ACT_EXIT_AIRBORNE:
        case ACT_SPECIAL_EXIT_AIRBORNE:
            return 1;
        default:
            break;
    }

    /* Dialogs/cutscene text can remain active while Mario's action changes
       briefly. Never let the COD proxy stomp the hidden Mario state while a
       native dialog is open. */
    if (get_dialog_id() != DIALOG_NONE) {
        return 1;
    }

    return 0;
}

static void set_bridge_controller_buttons(u16 down, u16 pressed) {
    if (gPlayer1Controller != NULL) {
        gPlayer1Controller->buttonDown = down;
        gPlayer1Controller->buttonPressed = pressed;
        gPlayer1Controller->rawStickX = 0;
        gPlayer1Controller->rawStickY = 0;
        gPlayer1Controller->stickX = 0.0f;
        gPlayer1Controller->stickY = 0.0f;
        gPlayer1Controller->stickMag = 0.0f;
    }
    /* SM64's dialog renderer reads controller 3, while Mario interactions read
       controller 1. Feed both so one COD Use press can start and advance NPC
       text without bypassing the native dialog state machine. */
    if (gPlayer3Controller != NULL) {
        gPlayer3Controller->buttonDown = down;
        gPlayer3Controller->buttonPressed = pressed;
        gPlayer3Controller->rawStickX = 0;
        gPlayer3Controller->rawStickY = 0;
        gPlayer3Controller->stickX = 0.0f;
        gPlayer3Controller->stickY = 0.0f;
        gPlayer3Controller->stickMag = 0.0f;
    }
}

static void apply_proxy(const struct Request *request) {
    int native_owns;
    u16 buttons = 0;

    if (gMarioState == NULL || gMarioObject == NULL) {
        return;
    }

    bridge_update_headless_dialog(request);
    native_owns = native_owns_mario_motion();
    gIw4lRunMarioAction = native_owns;

    if (!native_owns) {
        /*
         * COD owns normal locomotion. Only in this mode may the bridge replace
         * Mario's transform/collision cache with the external player state.
         * Previously this happened even inside cannons and dialog cutscenes,
         * pinning Mario to stale COD coordinates and trapping the action.
         */
        if (gMarioState->action != ACT_IDLE) {
            set_mario_action(gMarioState, ACT_IDLE, 0);
        }

        clear_mario_platform();
        gMarioObject->platform = NULL;

        gMarioState->pos[0] = request->pos[0];
        gMarioState->pos[1] = request->pos[1];
        gMarioState->pos[2] = request->pos[2];
        gMarioState->vel[0] = request->vel[0];
        gMarioState->vel[1] = request->vel[1];
        gMarioState->vel[2] = request->vel[2];
        gMarioState->faceAngle[1] = request->yaw;
        gMarioState->faceAngle[0] = request->pitch;
        gMarioState->input = 0;

        gMarioState->floorHeight = find_floor(
            gMarioState->pos[0],
            gMarioState->pos[1],
            gMarioState->pos[2],
            &gMarioState->floor
        );
        gMarioState->ceilHeight = find_ceil(
            gMarioState->pos[0],
            gMarioState->pos[1],
            gMarioState->pos[2],
            &gMarioState->ceil
        );
        gMarioState->waterLevel = find_water_level(
            gMarioState->pos[0],
            gMarioState->pos[2]
        );

        gMarioObject->oPosX = request->pos[0];
        gMarioObject->oPosY = request->pos[1];
        gMarioObject->oPosZ = request->pos[2];
        gMarioObject->oFaceAngleYaw = request->yaw;
        gMarioObject->oMoveAngleYaw = request->yaw;
        gMarioObject->header.gfx.pos[0] = request->pos[0];
        gMarioObject->header.gfx.pos[1] = request->pos[1];
        gMarioObject->header.gfx.pos[2] = request->pos[2];

        /*
         * update_objects() runs detect_object_collisions() before Mario's
         * behavior update. Refresh Mario's hitbox here, immediately after the
         * COD proxy move, so native enemies collide with the current COD
         * position rather than the previous frame's Mario bounds.
         */
        mario_update_hitbox_and_cap_model(gMarioState);
    }

    {
        const float yaw_s=sins(request->yaw);
        const float yaw_c=coss(request->yaw);
        const float pitch_s=sins(request->pitch);
        const float pitch_c=coss(request->pitch);
        const float eye_height=150.0f;
        const float look_distance=2000.0f;

        gIw4lRenderCameraPos[0]=gMarioState->pos[0];
        gIw4lRenderCameraPos[1]=gMarioState->pos[1]+eye_height;
        gIw4lRenderCameraPos[2]=gMarioState->pos[2];
        gIw4lRenderCameraFocus[0]=gIw4lRenderCameraPos[0]+yaw_s*pitch_c*look_distance;
        gIw4lRenderCameraFocus[1]=gIw4lRenderCameraPos[1]+pitch_s*look_distance;
        gIw4lRenderCameraFocus[2]=gIw4lRenderCameraPos[2]+yaw_c*pitch_c*look_distance;
        gIw4lUseExternalCamera=1;
    }

    /* COD Use is Mario B: starts native NPC/sign interactions and advances
       dialogs. Fire is Mario A only in a cannon or while a dialog is open. */
    if (request->attack_flags & BRIDGE_INPUT_USE) {
        buttons |= B_BUTTON;
    }
    if ((request->attack_flags & BRIDGE_INPUT_ATTACK) &&
        (gMarioState->action == ACT_IN_CANNON || get_dialog_id() != DIALOG_NONE)) {
        buttons |= A_BUTTON;
    }
    set_bridge_controller_buttons(buttons, buttons);

    if (gMarioState->action == ACT_IN_CANNON && gMarioState->usedObj != NULL) {
        s32 inputYaw = (s16)(request->yaw - gMarioState->usedObj->oMoveAngleYaw);
        if (inputYaw > 0x4000) inputYaw = 0x4000;
        if (inputYaw < -0x4000) inputYaw = -0x4000;
        gMarioObject->oMarioCannonInputYaw = inputYaw;
        gMarioState->faceAngle[0] = request->pitch;
    }
}

static int bridge_attackable_object(const struct Object *object) {
    int32_t model;
    if (object == NULL || object == gMarioObject) {
        return 0;
    }
    if ((object->activeFlags & ACTIVE_FLAG_ACTIVE) == 0) {
        return 0;
    }

    model = model_id_for_object(object);
    if (model < 0) {
        /* Skip logic-only helpers/spawners with no rendered graph node. */
        return 0;
    }

    switch (model) {
        case MODEL_YELLOW_COIN:
        case MODEL_YELLOW_COIN_NO_SHADOW:
        case MODEL_BLUE_COIN:
        case MODEL_BLUE_COIN_NO_SHADOW:
        case MODEL_RED_COIN:
        case MODEL_RED_COIN_NO_SHADOW:
        case MODEL_STAR:
        case MODEL_TRANSPARENT_STAR:
            return 0;
        default:
            break;
    }

    /*
     * Some behaviors populate oInteractType/hitboxes during their own update,
     * so don't require interactType to already be nonzero. But also avoid
     * targeting inert scenery with no interaction/hit volume at all.
     */
    if (object->oInteractType == 0 &&
        object->hitboxRadius <= 1.0f &&
        object->hurtboxRadius <= 1.0f) {
        return 0;
    }

    return 1;
}

static void apply_external_attack(const struct Request *request) {
    if ((request->attack_flags & BRIDGE_INPUT_ATTACK) == 0 || gObjectLists == NULL) {
        return;
    }
    /* In a cannon, fire belongs to Mario's A-button cannon action. During
       dialogs/cutscenes it belongs to the native state machine, never to the
       hitscan adapter. */
    if (gMarioState != NULL &&
        (gMarioState->action == ACT_IN_CANNON || get_dialog_id() != DIALOG_NONE)) {
        return;
    }

    /*
     * COD bullets are translated into the original SM64 object interaction
     * protocol. Behaviors already know how to react to INT_STATUS_WAS_ATTACKED
     * and the low-byte ATTACK_* value, so keep death/loot/state changes native.
     */
    const int ground_pound = request->pitch < -0x1800;
    struct Object *best = NULL;
    float best_score = 1.0e30f;
    int list_index;

    if (ground_pound) {
        for (list_index = 0; list_index < NUM_OBJ_LISTS; ++list_index) {
            struct ObjectNode *head = &gObjectLists[list_index];
            struct ObjectNode *node = head->next;
            while (node != head) {
                struct Object *object = (struct Object *)node;
                node = node->next;
                if (!bridge_attackable_object(object)) {
                    continue;
                }
                {
                    const float dx = object->oPosX - request->pos[0];
                    const float dz = object->oPosZ - request->pos[2];
                    const float down = request->pos[1] - object->oPosY;
                    const float horizontal2 = dx * dx + dz * dz;
                    if (down >= -80.0f && down <= 650.0f && horizontal2 <= 260.0f * 260.0f) {
                        const float score = horizontal2 + down * down * 0.15f;
                        if (score < best_score) {
                            best_score = score;
                            best = object;
                        }
                    }
                }
            }
        }
        if (best != NULL) {
            best->oInteractStatus |=
                INT_STATUS_INTERACTED |
                INT_STATUS_WAS_ATTACKED |
                ATTACK_GROUND_POUND_OR_TWIRL;
            return;
        }
    }

    {
        const float yaw_s = sins(request->yaw);
        const float yaw_c = coss(request->yaw);
        const float pitch_s = sins(request->pitch);
        const float pitch_c = coss(request->pitch);
        const float dir_x = yaw_s * pitch_c;
        const float dir_y = pitch_s;
        const float dir_z = yaw_c * pitch_c;
        const float max_range = 2600.0f;

        for (list_index = 0; list_index < NUM_OBJ_LISTS; ++list_index) {
            struct ObjectNode *head = &gObjectLists[list_index];
            struct ObjectNode *node = head->next;
            while (node != head) {
                struct Object *object = (struct Object *)node;
                node = node->next;
                if (!bridge_attackable_object(object)) {
                    continue;
                }

                {
                    const float dx = object->oPosX - request->pos[0];
                    const float dy = (object->oPosY + 80.0f) - request->pos[1];
                    const float dz = object->oPosZ - request->pos[2];
                    const float along = dx * dir_x + dy * dir_y + dz * dir_z;
                    float radius;
                    float px;
                    float py;
                    float pz;
                    float perp2;

                    if (along < 0.0f || along > max_range) {
                        continue;
                    }

                    px = dx - dir_x * along;
                    py = dy - dir_y * along;
                    pz = dz - dir_z * along;
                    perp2 = px * px + py * py + pz * pz;
                    radius = object->hitboxRadius > 1.0f ? object->hitboxRadius : 120.0f;
                    radius += 55.0f;

                    if (perp2 <= radius * radius) {
                        const float score = along + perp2 * 0.002f;
                        if (score < best_score) {
                            best_score = score;
                            best = object;
                        }
                    }
                }
            }
        }
    }

    if (best != NULL) {
        best->oInteractStatus |=
            INT_STATUS_INTERACTED |
            INT_STATUS_WAS_ATTACKED |
            ATTACK_FAST_ATTACK;
    }
}


/*
 * COD locomotion is mirrored into gMarioObject immediately before native
 * update_objects(). The original collision/interaction path normally owns
 * enemy contact damage. Keep a conservative fallback for the external-player
 * mode because some actor hitboxes are initialized during their behavior
 * update, after SM64's collision pass has already run for that frame.
 *
 * Only apply this when the vanilla interaction pass did not change health.
 * This preserves native damage whenever it works and prevents a missed first
 * collision frame from making enemies harmless forever.
 */
static void bridge_contact_damage_fallback(int32_t health_before) {
    const uint32_t harmful =
        INTERACT_DAMAGE |
        INTERACT_BOUNCE_TOP |
        INTERACT_BOUNCE_TOP2 |
        INTERACT_BULLY |
        INTERACT_FLAME |
        INTERACT_MR_BLIZZARD |
        INTERACT_HIT_FROM_BELOW |
        INTERACT_CLAM_OR_BUBBA |
        INTERACT_SNUFIT_BULLET |
        INTERACT_SHOCK;
    struct Object *best = NULL;
    float best_distance2 = 1.0e30f;
    int list_index;

    if (gMarioState == NULL || gMarioObject == NULL || gObjectLists == NULL) {
        return;
    }
    if (gMarioState->health != health_before || gMarioState->invincTimer != 0) {
        return;
    }

    for (list_index = 0; list_index < NUM_OBJ_LISTS; ++list_index) {
        struct ObjectNode *head = &gObjectLists[list_index];
        struct ObjectNode *node = head->next;
        while (node != head) {
            struct Object *object = (struct Object *)node;
            float mario_bottom;
            float mario_top;
            float object_bottom;
            float object_top;
            float dx;
            float dz;
            float gfx_dx;
            float gfx_dz;
            float radius;
            float distance2;
            float gfx_distance2;
            float object_y;

            node = node->next;
            {
                const int32_t model = model_id_for_object(object);
                const int known_enemy = model == MODEL_GOOMBA;
                const int authored_harm =
                    (object->oInteractType & harmful) != 0 ||
                    object->oDamageOrCoinValue > 0;
                float mario_radius;
                float object_radius;

                /*
                 * Actor interaction fields are populated at different points
                 * in the native behavior update. Requiring BOTH a harmful
                 * interact bit and a positive damage value made the external
                 * player miss actors whose damage metadata was one frame ahead
                 * of their interact mask. Either authored signal is enough.
                 * Goombas remain an explicit native-model fallback because
                 * their first active frame can have neither field seated yet.
                 */
                if (object == gMarioObject ||
                    (object->activeFlags & ACTIVE_FLAG_ACTIVE) == 0 ||
                    (!known_enemy && object->oIntangibleTimer != 0) ||
                    (!known_enemy && !authored_harm)) {
                    continue;
                }

                /*
                 * External COD motion can meet an actor on the same native tick
                 * that actor initializes its interaction hitbox. Vanilla's
                 * collision pass then sees a zero/stale radius for one frame.
                 * Known enemies get conservative SM64-sized fallback bounds so
                 * walking into a Goomba cannot become harmless.
                 */
                mario_radius = gMarioObject->hitboxRadius;
                if (mario_radius < 60.0f) mario_radius = 60.0f;
                object_radius = object->hitboxRadius;
                if (object->hurtboxRadius > object_radius) {
                    object_radius = object->hurtboxRadius;
                }
                if (known_enemy && object_radius < 120.0f) {
                    object_radius = 120.0f;
                }
                radius = mario_radius + object_radius;
                if (radius <= 0.0f) {
                    continue;
                }
            }
            /*
             * Behaviors are allowed to offset header.gfx.pos from oPos. The
             * player sees and walks into the graphics node, so use whichever
             * native position is closer to the externally controlled Mario.
             * This also covers the first update where one of the two positions
             * is still stale.
             */
            dx = gMarioObject->oPosX - object->oPosX;
            dz = gMarioObject->oPosZ - object->oPosZ;
            distance2 = dx * dx + dz * dz;
            gfx_dx = gMarioObject->oPosX - object->header.gfx.pos[0];
            gfx_dz = gMarioObject->oPosZ - object->header.gfx.pos[2];
            gfx_distance2 = gfx_dx * gfx_dx + gfx_dz * gfx_dz;
            object_y = object->oPosY;
            if (gfx_distance2 < distance2) {
                distance2 = gfx_distance2;
                object_y = object->header.gfx.pos[1];
            }
            if (distance2 >= radius * radius) {
                continue;
            }

            mario_bottom = gMarioObject->oPosY - gMarioObject->hitboxDownOffset;
            mario_top = mario_bottom +
                (gMarioObject->hitboxHeight > 1.0f ? gMarioObject->hitboxHeight : 160.0f);
            object_bottom = object_y - object->hitboxDownOffset;
            object_top = object_bottom +
                (object->hitboxHeight > 1.0f ? object->hitboxHeight :
                    (model_id_for_object(object) == MODEL_GOOMBA ? 160.0f : 0.0f));
            if (mario_bottom > object_top || mario_top < object_bottom) {
                continue;
            }

            if (distance2 < best_distance2) {
                best_distance2 = distance2;
                best = object;
            }
        }
    }

    if (best != NULL) {
        int damage = best->oDamageOrCoinValue;
        if (damage <= 0 &&
            ((best->oInteractType & harmful) != 0 ||
             model_id_for_object(best) == MODEL_GOOMBA)) {
            damage = 1;
        }
        if (!(gMarioState->flags & MARIO_CAP_ON_HEAD)) {
            damage += (damage + 1) / 2;
        }
        if (gMarioState->flags & MARIO_METAL_CAP) {
            damage = 0;
        }

        if (damage > 0) {
            const int32_t amount = damage * 0x100;
            gMarioState->health -= amount;
            if (gMarioState->health < 0) {
                gMarioState->health = 0;
            }
            gMarioState->invincTimer = 30;
            best->oInteractStatus |= INT_STATUS_INTERACTED | INT_STATUS_ATTACKED_MARIO;
            play_sound(SOUND_MARIO_ATTACKED, gMarioObject->header.gfx.cameraToObject);
            fprintf(
                stderr,
                "iw4l-sm64-native: contact damage fallback model=%d damage=%d health=%d\n",
                (int)model_id_for_object(best),
                damage,
                (int)gMarioState->health
            );
            fflush(stderr);
        }
    }
}

void iw4l_sm64_capture_begin_frame(void) {
    gIw4lRenderTriangleCount = 0;
}

void iw4l_sm64_capture_triangle(
    const float *pos9,
    const float *uv6,
    const uint8_t *rgba12,
    uint32_t texture_id,
    int textured,
    int alpha,
    uint8_t wrap_s,
    uint8_t wrap_t
) {
    struct Iw4lSm64RenderTriangle *out;
    if (gIw4lRenderTriangleCount >= IW4L_SM64_MAX_RENDER_TRIANGLES) {
        return;
    }
    out = &gIw4lRenderTriangles[gIw4lRenderTriangleCount++];
    memcpy(out->pos,pos9,sizeof(out->pos));
    memcpy(out->uv,uv6,sizeof(out->uv));
    memcpy(out->rgba,rgba12,sizeof(out->rgba));
    out->texture_id=texture_id;
    out->textured=textured ? 1 : 0;
    out->alpha=alpha ? 1 : 0;
    out->wrap_s=wrap_s;
    out->wrap_t=wrap_t;
}

void iw4l_sm64_capture_texture(
    uint32_t id,
    const uint8_t *rgba,
    uint32_t width,
    uint32_t height
) {
    uint32_t i;
    struct Iw4lSm64TextureStorage *slot = NULL;
    size_t bytes;

    if (rgba == NULL || width == 0 || height == 0) {
        return;
    }

    for (i=0;i<gIw4lTextureCount;i++) {
        if (gIw4lTextureStorage[i].view.id == id) {
            slot=&gIw4lTextureStorage[i];
            break;
        }
    }
    if (slot == NULL) {
        if (gIw4lTextureCount >= IW4L_SM64_MAX_TEXTURES) {
            return;
        }
        slot=&gIw4lTextureStorage[gIw4lTextureCount++];
        memset(slot,0,sizeof(*slot));
        slot->view.id=id;
    }

    bytes=(size_t)width*(size_t)height*4u;
    if (slot->capacity < bytes) {
        uint8_t *next=(uint8_t *)realloc(slot->owned_rgba,bytes);
        if (next == NULL) {
            return;
        }
        slot->owned_rgba=next;
        slot->capacity=bytes;
    }
    memcpy(slot->owned_rgba,rgba,bytes);
    slot->view.width=width;
    slot->view.height=height;
    slot->view.generation++;
    slot->view.rgba=slot->owned_rgba;
}

static uint32_t collect_objects(const struct Object **objects, uint32_t capacity) {
    uint32_t count = 0;
    int list_index;
    if (gObjectLists == NULL) {
        return 0;
    }

    for (list_index = 0; list_index < NUM_OBJ_LISTS; ++list_index) {
        struct ObjectNode *head = &gObjectLists[list_index];
        struct ObjectNode *node = head->next;
        while (node != head) {
            struct Object *object = (struct Object *)node;
            node = node->next;
            if (object == gMarioObject) {
                continue;
            }
            if ((object->activeFlags & ACTIVE_FLAG_ACTIVE) == 0) {
                continue;
            }
            if (count < capacity) {
                objects[count++] = object;
            }
        }
    }
    return count;
}

static uint32_t collect_dynamic_surfaces(
    const struct Surface **out,
    uint32_t capacity
) {
    const struct Surface *seen[8192];
    uint32_t seen_count = 0;
    int z, x, partition;

    for (z = 0; z < NUM_CELLS; ++z) {
        for (x = 0; x < NUM_CELLS; ++x) {
            for (partition = 0; partition < 3; ++partition) {
                struct SurfaceNode *node =
                    gDynamicSurfacePartition[z][x][partition].next;
                uint32_t guard = 0;

                while (node != NULL) {
                    const struct Surface *surface;
                    uint32_t i;
                    int duplicate = 0;

                    if (++guard > 4096) {
                        fprintf(
                            stderr,
                            "iw4l-sm64-native: warning: dynamic surface chain guard tripped at cell=(%d,%d) partition=%d\n",
                            x, z, partition
                        );
                        break;
                    }

                    surface=node->surface;
                    if (surface == NULL) {
                        fprintf(
                            stderr,
                            "iw4l-sm64-native: warning: null dynamic surface at cell=(%d,%d) partition=%d\n",
                            x, z, partition
                        );
                        break;
                    }

#ifndef USE_SYSTEM_MALLOC
                    /*
                     * With the original fixed pools, reject corrupted pointers
                     * before dereferencing their vertices. Dynamic surfaces
                     * must live inside sSurfacePool.
                     */
                    if (surface < sSurfacePool ||
                        surface >= sSurfacePool + sSurfacePoolSize) {
                        fprintf(
                            stderr,
                            "iw4l-sm64-native: warning: dynamic surface pointer outside pool at cell=(%d,%d) partition=%d\n",
                            x, z, partition
                        );
                        break;
                    }
#endif

                    for (i = 0; i < seen_count; ++i) {
                        if (seen[i] == surface) {
                            duplicate = 1;
                            break;
                        }
                    }

                    if (!duplicate) {
                        if (seen_count >= capacity) {
                            return seen_count;
                        }
                        seen[seen_count++] = surface;
                    }

                    node=node->next;
                }
            }
        }
    }

    for (uint32_t i = 0; i < seen_count; ++i) {
        out[i] = seen[i];
    }
    return seen_count;
}

static void print_native_census_once(
    const struct Object **objects,
    uint32_t count,
    uint32_t dynamic_count
) {
    static int printed = 0;
    uint32_t trees = 0;
    uint32_t yellow_coins = 0;
    uint32_t red_coins = 0;
    uint32_t cannons = 0;
    uint32_t stars = 0;
    uint32_t goombas = 0;
    uint32_t i;

    if (printed) {
        return;
    }
    printed = 1;

    for (i = 0; i < count; ++i) {
        int32_t model = model_id_for_object(objects[i]);
        if (model == MODEL_BOB_BUBBLY_TREE) trees++;
        if (model == MODEL_YELLOW_COIN || model == MODEL_YELLOW_COIN_NO_SHADOW) yellow_coins++;
        if (model == MODEL_RED_COIN || model == MODEL_RED_COIN_NO_SHADOW) red_coins++;
        if (model == MODEL_CANNON_BASE || model == MODEL_DL_CANNON_LID) cannons++;
        if (model == MODEL_STAR || model == MODEL_TRANSPARENT_STAR) stars++;
        if (model == MODEL_GOOMBA) goombas++;
    }

    fprintf(
        stderr,
        "iw4l-sm64-native: census objects=%u trees=%u yellow_coins=%u red_coins=%u cannons=%u stars=%u goombas=%u dynamic_surfaces=%u\n",
        count,
        trees,
        yellow_coins,
        red_coins,
        cannons,
        stars,
        goombas,
        dynamic_count
    );
}

static uint16_t bridge_dialog_text(char *out, uint16_t capacity, int16_t *dialog_id) {
    void **dialog_table;
    struct DialogEntry *dialog;
    const u8 *src;
    uint16_t n = 0;
    s16 id = get_dialog_id();

    *dialog_id = id;
    if (id == DIALOG_NONE || capacity == 0) {
        return 0;
    }

    dialog_table = segmented_to_virtual(seg2_dialog_table);
    dialog = segmented_to_virtual(dialog_table[id]);
    if (dialog == NULL) {
        return 0;
    }
    src = segmented_to_virtual(dialog->str);
    if (src == NULL) {
        return 0;
    }

    while (*src != DIALOG_CHAR_TERMINATOR && n + 1 < capacity) {
        u8 ch = *src++;
        char ascii = '?';
        if (ch <= 0x09) ascii = (char)('0' + ch);
        else if (ch >= 0x0A && ch <= 0x23) ascii = (char)('A' + (ch - 0x0A));
        else if (ch >= 0x24 && ch <= 0x3D) ascii = (char)('a' + (ch - 0x24));
        else if (ch == 0x3E) ascii = '\'';
        else if (ch == 0x3F || ch == 0x6E) ascii = '.';
        else if (ch == 0x6F) ascii = ',';
        else if (ch == 0x9E) ascii = ' ';
        else if (ch == 0x9F) ascii = '-';
        else if (ch == 0xD0) ascii = '/';
        else if (ch == 0xD1) {
            const char *s = "the";
            while (*s && n + 1 < capacity) out[n++] = *s++;
            continue;
        } else if (ch == 0xD2) {
            const char *s = "you";
            while (*s && n + 1 < capacity) out[n++] = *s++;
            continue;
        } else if (ch == 0xE1) ascii = '(';
        else if (ch == 0xE3) ascii = ')';
        else if (ch == 0xE4) ascii = '+';
        else if (ch == 0xE5) ascii = '&';
        else if (ch == 0xE6) ascii = ':';
        else if (ch == 0xF2) ascii = '!';
        else if (ch == 0xF3) ascii = '%';
        else if (ch == 0xF4) ascii = '?';
        else if (ch == 0xF5 || ch == 0xF6) ascii = '"';
        else if (ch == 0xF7) ascii = '~';
        else if (ch == 0xF8) {
            const char *s = "...";
            while (*s && n + 1 < capacity) out[n++] = *s++;
            continue;
        } else if (ch == 0xF9) ascii = '$';
        else if (ch == 0xFE) ascii = '\n';
        out[n++] = ascii;
    }
    out[n] = '\0';
    return n;
}

static const struct Iw4lSm64SnapshotView *fill_snapshot_view(void) {
    const struct Object *objects[4096];
    const struct Surface *dynamic_surfaces[8192];
    uint32_t count;
    uint32_t dynamic_count;
    uint32_t i;
    int16_t dialog_id = DIALOG_NONE;
    uint16_t dialog_text_len;

    gBridgeStage = "collect_objects";
    count = collect_objects(objects, 4096);
    gBridgeStage = "collect_dynamic_surfaces";
    dynamic_count = collect_dynamic_surfaces(dynamic_surfaces, 8192);
    print_native_census_once(objects, count, dynamic_count);

    dialog_text_len = bridge_dialog_text(
        gIw4lDialogText,
        (uint16_t)sizeof(gIw4lDialogText),
        &dialog_id
    );

    memset(&gIw4lSnapshot, 0, sizeof(gIw4lSnapshot));
    gIw4lSnapshot.abi_version = IW4L_SM64_ABI_VERSION;
    gIw4lSnapshot.tick = gGlobalTimer;
    gIw4lSnapshot.mario_health = gMarioState != NULL ? gMarioState->health : 0;
    gIw4lSnapshot.coins = gMarioState != NULL ? gMarioState->numCoins : 0;
    gIw4lSnapshot.mario_action = gMarioState != NULL ? gMarioState->action : 0;
    if (gMarioState != NULL) {
        for (i = 0; i < 3; ++i) {
            gIw4lSnapshot.mario_pos[i] = gMarioState->pos[i];
            gIw4lSnapshot.mario_vel[i] = gMarioState->vel[i];
        }
        gIw4lSnapshot.mario_yaw = gMarioState->faceAngle[1];
    }
    gIw4lSnapshot.dialog_id = dialog_id;
    gIw4lSnapshot.dialog_text = gIw4lDialogText;
    gIw4lSnapshot.dialog_text_len = dialog_text_len;

    for (i = 0; i < count; ++i) {
        const struct Object *object = objects[i];
        struct Iw4lSm64Object *out = &gIw4lObjects[i];
        uint32_t axis;
        memset(out, 0, sizeof(*out));
        out->id = object_id(object);
        out->model_id = model_id_for_object(object);
        for (axis = 0; axis < 3; ++axis) {
            out->pos[axis] = object->header.gfx.pos[axis];
            out->face_angle[axis] = object->header.gfx.angle[axis];
            out->scale[axis] = object->header.gfx.scale[axis];
        }
        out->active_flags = (uint16_t)object->activeFlags;
        out->render_flags = (uint16_t)object->header.gfx.node.flags;
        out->anim_id = object->header.gfx.animInfo.animID;
        out->anim_frame = object->header.gfx.animInfo.animFrame;
        out->anim_state = (int32_t)object->oAnimState;
        out->interact_status = (uint32_t)object->oInteractStatus;
        out->damage_or_coin_value = (int32_t)object->oDamageOrCoinValue;
    }
    gIw4lSnapshot.objects = gIw4lObjects;
    gIw4lSnapshot.object_count = count;

    for (i = 0; i < dynamic_count; ++i) {
        const struct Surface *surface = dynamic_surfaces[i];
        float *v = gIw4lDynamicSurfaces[i].vertices;
        v[0] = (float)surface->vertex1[0];
        v[1] = (float)surface->vertex1[1];
        v[2] = (float)surface->vertex1[2];
        v[3] = (float)surface->vertex2[0];
        v[4] = (float)surface->vertex2[1];
        v[5] = (float)surface->vertex2[2];
        v[6] = (float)surface->vertex3[0];
        v[7] = (float)surface->vertex3[1];
        v[8] = (float)surface->vertex3[2];
    }
    gIw4lSnapshot.dynamic_surfaces = gIw4lDynamicSurfaces;
    gIw4lSnapshot.dynamic_surface_count = dynamic_count;
    gIw4lSnapshot.render_triangles = gIw4lRenderTriangles;
    gIw4lSnapshot.render_triangle_count = gIw4lRenderTriangleCount;

    for (i=0;i<gIw4lTextureCount;i++) {
        gIw4lTextureViews[i]=gIw4lTextureStorage[i].view;
    }
    gIw4lSnapshot.textures=gIw4lTextureViews;
    gIw4lSnapshot.texture_count=gIw4lTextureCount;
    gIw4lSnapshot.audio_samples=gIw4lAudioSamples;
    gIw4lSnapshot.audio_frame_count=gIw4lAudioFrameCount;
    gIw4lSnapshot.audio_sample_rate=IW4L_AUDIO_SAMPLE_RATE;
    gIw4lSnapshot.audio_channels=IW4L_AUDIO_CHANNELS;
    gIw4lSnapshot.audio_reserved=0;
    return &gIw4lSnapshot;
}

static void write_snapshot(void) {
    const struct Object *objects[4096];
    const struct Surface *dynamic_surfaces[8192];
    uint32_t count;
    uint32_t dynamic_count;
    uint32_t i;
    int16_t dialog_id = DIALOG_NONE;
    uint16_t dialog_text_len = 0;
    char dialog_text[4096];
    static uint32_t snapshot_number = 0;

    snapshot_number++;

    if (snapshot_number <= 3 || (snapshot_number % 30u) == 0u) {
        fprintf(stderr, "iw4l-sm64-native: snapshot %u collecting objects\n", snapshot_number);
        fflush(stderr);
    }
    gBridgeStage = "collect_objects";
    count = collect_objects(objects, 4096);

    if (snapshot_number <= 3 || (snapshot_number % 30u) == 0u) {
        fprintf(stderr, "iw4l-sm64-native: snapshot %u objects=%u; collecting dynamic surfaces\n",
                snapshot_number, count);
        fflush(stderr);
    }
    gBridgeStage = "collect_dynamic_surfaces";
    dynamic_count = collect_dynamic_surfaces(dynamic_surfaces, 8192);

    if (snapshot_number <= 3 || (snapshot_number % 30u) == 0u) {
        fprintf(stderr, "iw4l-sm64-native: snapshot %u dynamic_surfaces=%u\n",
                snapshot_number, dynamic_count);
        fflush(stderr);
    }

    print_native_census_once(objects, count, dynamic_count);
    dialog_text_len = bridge_dialog_text(dialog_text, sizeof(dialog_text), &dialog_id);

    gBridgeStage = "write_snapshot";
    write_u32(SNAPSHOT_MAGIC);
    write_u32(gGlobalTimer);
    write_u32(count);
    write_u32(dynamic_count);
    write_i32(gMarioState != NULL ? gMarioState->health : 0);
    write_i32(gMarioState != NULL ? gMarioState->numCoins : 0);
    write_u32(gMarioState != NULL ? gMarioState->action : 0);
    write_f32(gMarioState != NULL ? gMarioState->pos[0] : 0.0f);
    write_f32(gMarioState != NULL ? gMarioState->pos[1] : 0.0f);
    write_f32(gMarioState != NULL ? gMarioState->pos[2] : 0.0f);
    write_f32(gMarioState != NULL ? gMarioState->vel[0] : 0.0f);
    write_f32(gMarioState != NULL ? gMarioState->vel[1] : 0.0f);
    write_f32(gMarioState != NULL ? gMarioState->vel[2] : 0.0f);
    write_i16(gMarioState != NULL ? gMarioState->faceAngle[1] : 0);
    write_i16(dialog_id);
    write_u16(dialog_text_len);
    if (dialog_text_len > 0) {
        fwrite(dialog_text, 1, dialog_text_len, stdout);
    }

    for (i = 0; i < count; ++i) {
        const struct Object *object = objects[i];
        write_u32(object_id(object));
        write_i32(model_id_for_object(object));

        /*
         * Presentation must follow the native graphics node, not the gameplay
         * hitbox origin. SM64 behaviors intentionally offset gfx.pos for moving
         * platforms, bosses and multipart objects. External attacks still use
         * oPos/hitbox fields directly above in the bridge.
         */
        write_f32(object->header.gfx.pos[0]);
        write_f32(object->header.gfx.pos[1]);
        write_f32(object->header.gfx.pos[2]);
        write_i16(object->header.gfx.angle[0]);
        write_i16(object->header.gfx.angle[1]);
        write_i16(object->header.gfx.angle[2]);
        write_f32(object->header.gfx.scale[0]);
        write_f32(object->header.gfx.scale[1]);
        write_f32(object->header.gfx.scale[2]);
        write_u16((uint16_t)object->activeFlags);
        write_u16((uint16_t)object->header.gfx.node.flags);
        write_i16(object->header.gfx.animInfo.animID);
        write_i16(object->header.gfx.animInfo.animFrame);
        write_i32((int32_t)object->oAnimState);
        write_u32((uint32_t)object->oInteractStatus);
        write_i32((int32_t)object->oDamageOrCoinValue);
    }

    for (i = 0; i < dynamic_count; ++i) {
        const struct Surface *surface = dynamic_surfaces[i];
        write_f32((float)surface->vertex1[0]);
        write_f32((float)surface->vertex1[1]);
        write_f32((float)surface->vertex1[2]);
        write_f32((float)surface->vertex2[0]);
        write_f32((float)surface->vertex2[1]);
        write_f32((float)surface->vertex2[2]);
        write_f32((float)surface->vertex3[0]);
        write_f32((float)surface->vertex3[1]);
        write_f32((float)surface->vertex3[2]);
    }

    fflush(stdout);
}

IW4L_SM64_API uint32_t iw4l_sm64_abi_version(void) {
    return IW4L_SM64_ABI_VERSION;
}

IW4L_SM64_API const char *iw4l_sm64_last_error(void) {
    return gIw4lLastError;
}

IW4L_SM64_API int iw4l_sm64_init(const char *level_name, int area, int act) {
    const struct LevelSpec *level;

    if (gIw4lInitialized) {
        snprintf(gIw4lLastError, sizeof(gIw4lLastError), "SM64 runtime already initialized");
        return 0;
    }
    if (level_name == NULL || level_name[0] == '\0') {
        level_name = "bob";
    }

    level = find_level(level_name);
    if (level == NULL) {
        snprintf(gIw4lLastError, sizeof(gIw4lLastError), "unknown level '%s'", level_name);
        return 0;
    }

#ifdef USE_SYSTEM_MALLOC
    gBridgeStage = "main_pool_init";
    main_pool_init();
    gGfxAllocOnlyPool = alloc_only_pool_init();
#else
    {
        static u64 pool[0x165000/8 / 4 * sizeof(void *)];
        main_pool_init(pool, pool + sizeof(pool) / sizeof(pool[0]));
    }
#endif
    gEffectsMemoryPool = mem_pool_init(0x4000, MEMORY_POOL_LEFT);

    gfx_init(
        &gfx_dummy_wm_api,
        &gfx_dummy_renderer_api,
        "IW4L Embedded SM64",
        0
    );
    gBridgeStage = "audio_backend_init";
    bridge_audio_backend_init();
    gBridgeStage = "audio_init";
    audio_init();
    gBridgeStage = "sound_init";
    sound_init();
    gBridgeStage = "thread5_game_loop";
    thread5_game_loop(NULL);

    gCurrSaveFileNum = 1;
    gCurrActNum = (s16)act;
    gCurrLevelNum = level->level;
    gCurrCourseNum = level->course;
    save_file_set_flags(SAVE_FLAG_FILE_EXISTS);

    gIw4lLevelCommand = (struct LevelCommand *)level->entry;
    gDebugLevelSelect = TRUE;
    gBridgeStage = "initial_select_gfx_pool";
    select_gfx_pool();
    gBridgeStage = "initial_level_script_execute";
    gIw4lLevelCommand = level_script_execute(gIw4lLevelCommand);
    gGlobalTimer++;
    gDebugLevelSelect = FALSE;

    if (gCurrentArea == NULL || gMarioState == NULL || gMarioObject == NULL) {
        snprintf(
            gIw4lLastError,
            sizeof(gIw4lLastError),
            "level '%s' did not initialize area/player (area=%d act=%d)",
            level_name,
            area,
            act
        );
        gIw4lLevelCommand = NULL;
        return 0;
    }

    if ((gMarioState->action & ACT_GROUP_MASK) == ACT_GROUP_CUTSCENE) {
        set_mario_action(gMarioState, ACT_IDLE, 0);
    }

    gIw4lInitialized = 1;
    gIw4lLastError[0] = '\0';
    gBridgeStage = "course_ready";
    fprintf(
        stderr,
        "iw4l-sm64-native: embedded decomp ready: level=%s area=%d act=%d action=0x%08X\n",
        level_name,
        area,
        act,
        gMarioState->action
    );
    fflush(stderr);
    return 1;
}

IW4L_SM64_API const struct Iw4lSm64SnapshotView *iw4l_sm64_step(
    const struct Iw4lSm64PlayerProxy *player
) {
    struct Request request;
    int32_t health_before = gMarioState != NULL ? gMarioState->health : 0;

    if (!gIw4lInitialized || gIw4lLevelCommand == NULL || player == NULL) {
        snprintf(gIw4lLastError, sizeof(gIw4lLastError), "SM64 runtime is not initialized");
        return NULL;
    }

    memset(&request, 0, sizeof(request));
    request.op = OP_STEP;
    memcpy(request.pos, player->pos, sizeof(request.pos));
    memcpy(request.vel, player->vel, sizeof(request.vel));
    request.yaw = player->yaw;
    request.pitch = player->pitch;
    request.health = player->health;
    request.attack_flags = player->attack_flags;

    gBridgeStage = "apply_proxy";
    apply_proxy(&request);
    gBridgeStage = "apply_external_attack";
    apply_external_attack(&request);
    gBridgeStage = "select_gfx_pool";
    select_gfx_pool();
    gBridgeStage = "level_script_execute";
    gIw4lLevelCommand = level_script_execute(gIw4lLevelCommand);
    gGlobalTimer++;
    gBridgeStage = "contact_damage_fallback";
    bridge_contact_damage_fallback(health_before);
    gBridgeStage = "audio_tick";
    bridge_audio_tick();
    if (gMarioState != NULL && gMarioState->health != health_before) {
        fprintf(
            stderr,
            "iw4l-sm64-native: Mario health %d -> %d (delta=%d)\n",
            (int)health_before,
            (int)gMarioState->health,
            (int)(gMarioState->health - health_before)
        );
        fflush(stderr);
    }
    gBridgeStage = "snapshot";
    return fill_snapshot_view();
}

IW4L_SM64_API void iw4l_sm64_shutdown(void) {
    {
        uint32_t i;
        for (i=0;i<gIw4lTextureCount;i++) {
            free(gIw4lTextureStorage[i].owned_rgba);
            memset(&gIw4lTextureStorage[i],0,sizeof(gIw4lTextureStorage[i]));
        }
        gIw4lTextureCount=0;
        gIw4lRenderTriangleCount=0;
    }
    gIw4lInitialized = 0;
    gIw4lAudioFrameCount = 0;
    gIw4lAudioCadence = 0;
    gIw4lRunMarioAction = 0;
    gIw4lUseExternalCamera = 0;
    gIw4lLevelCommand = NULL;
    gBridgeDialogPendingReset = 0;
    memset(&gIw4lSnapshot, 0, sizeof(gIw4lSnapshot));
    gBridgeStage = "shutdown";
}

#ifdef _WIN32
static LONG WINAPI bridge_exception_filter(EXCEPTION_POINTERS *info) {
    DWORD code = info != NULL && info->ExceptionRecord != NULL
        ? info->ExceptionRecord->ExceptionCode
        : 0;
    void *address = info != NULL && info->ExceptionRecord != NULL
        ? info->ExceptionRecord->ExceptionAddress
        : NULL;

    fprintf(
        stderr,
        "iw4l-sm64-native: FATAL native exception code=0x%08lX address=%p globalTimer=%u stage=%s\n",
        (unsigned long)code,
        address,
        (unsigned)gGlobalTimer,
        gBridgeStage != NULL ? (const char *)gBridgeStage : "unknown"
    );
    fflush(stderr);
    return EXCEPTION_EXECUTE_HANDLER;
}
#endif

#ifndef IW4L_SM64_EMBEDDED
static int bridge_main(int argc, char **argv) {
#ifdef _WIN32
    SetUnhandledExceptionFilter(bridge_exception_filter);
#endif
    const char *level_name = "bob";
    int area = 1;
    int act = 1;
    int i;
    const struct LevelSpec *level;
    struct LevelCommand *level_command;

#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
#endif

    for (i = 1; i + 1 < argc; ++i) {
        if (strcmp(argv[i], "--level") == 0) {
            level_name = argv[++i];
        } else if (strcmp(argv[i], "--area") == 0) {
            area = atoi(argv[++i]);
        } else if (strcmp(argv[i], "--act") == 0) {
            act = atoi(argv[++i]);
        }
    }

    level = find_level(level_name);
    if (level == NULL) {
        fprintf(stderr, "iw4l-sm64-native: unknown level '%s'\n", level_name);
        return 2;
    }

#ifdef USE_SYSTEM_MALLOC
    gBridgeStage = "main_pool_init";
    main_pool_init();
    gGfxAllocOnlyPool = alloc_only_pool_init();
#else
    static u64 pool[0x165000/8 / 4 * sizeof(void *)];
    main_pool_init(pool, pool + sizeof(pool) / sizeof(pool[0]));
#endif
    gEffectsMemoryPool = mem_pool_init(0x4000, MEMORY_POOL_LEFT);

    gfx_init(
        &gfx_dummy_wm_api,
        &gfx_dummy_renderer_api,
        "IW4L SM64 Gameplay Bridge",
        0
    );
    gBridgeStage = "audio_backend_init";
    bridge_audio_backend_init();
    gBridgeStage = "audio_init";
    audio_init();
    gBridgeStage = "sound_init";
    sound_init();
    gBridgeStage = "thread5_game_loop";
    thread5_game_loop(NULL);

    gCurrSaveFileNum = 1;
    gCurrActNum = (s16)act;
    gCurrLevelNum = level->level;
    gCurrCourseNum = level->course;

    /*
     * The bridge jumps directly into a course. A blank/fresh SM64 save causes
     * init_level() to put Mario into ACT_INTRO_CUTSCENE, which is the castle
     * opening state machine and is invalid inside BOB/WF/etc. Mark file 1 as
     * existing before course initialization so the native decomp selects its
     * normal in-course idle path instead.
     */
    save_file_set_flags(SAVE_FLAG_FILE_EXISTS);

    /*
     * Do not pre-unlock course cannons. Native SM64 dialogue is now bridged to
     * the COD UI, so Bob-omb Buddy and the original save/progression behavior
     * own cannon unlocking again.
     */

    level_command = (struct LevelCommand *)level->entry;

    /*
     * Bypass only the act-select UI during the cold start. The normal main
     * scripts still load all shared graph nodes and dispatch into the requested
     * course. init_mario() remains responsible for Mario's native state.
     */
    gDebugLevelSelect = TRUE;

    /* First execute loads shared assets, the area, and reaches its CALL_LOOP. */
    gBridgeStage = "initial_select_gfx_pool";
    select_gfx_pool();
    gBridgeStage = "initial_level_script_execute";
    level_command = level_script_execute(level_command);
    gGlobalTimer++;
    gDebugLevelSelect = FALSE;
    gBridgeStage = "course_ready";

    if (gCurrentArea == NULL || gMarioState == NULL || gMarioObject == NULL) {
        fprintf(stderr,
                "iw4l-sm64-native: level '%s' did not initialize area/player (area=%d act=%d)\n",
                level_name, area, act);
        return 3;
    }

    if ((gMarioState->action & ACT_GROUP_MASK) == ACT_GROUP_CUTSCENE) {
        fprintf(
            stderr,
            "iw4l-sm64-native: normalizing unexpected startup cutscene action 0x%08X to ACT_IDLE\n",
            gMarioState->action
        );
        set_mario_action(gMarioState, ACT_IDLE, 0);
    }

    fprintf(stderr,
            "iw4l-sm64-native: native decomp gameplay ready: level=%s area=%d act=%d action=0x%08X\n",
            level_name, area, act, gMarioState->action);

    for (;;) {
        struct Request request;
        gBridgeStage = "read_request";
        if (!read_request(&request)) {
            break;
        }
        if (request.op == OP_SHUTDOWN) {
            break;
        }
        if (request.op != OP_STEP) {
            continue;
        }

        {
            static uint32_t bridge_step = 0;
            bridge_step++;

            if (bridge_step <= 3 || (bridge_step % 30u) == 0u) {
                fprintf(stderr, "iw4l-sm64-native: step %u begin\n", bridge_step);
                fflush(stderr);
            }

            gBridgeStage = "apply_proxy";
            apply_proxy(&request);
            gBridgeStage = "apply_external_attack";
            apply_external_attack(&request);

            if (bridge_step <= 3 || (bridge_step % 30u) == 0u) {
                fprintf(stderr, "iw4l-sm64-native: step %u proxy applied\n", bridge_step);
                fflush(stderr);
            }

            gBridgeStage = "select_gfx_pool";
            select_gfx_pool();
            gBridgeStage = "level_script_execute";
            level_command = level_script_execute(level_command);
            gGlobalTimer++;

            if (bridge_step <= 3 || (bridge_step % 30u) == 0u) {
                fprintf(stderr, "iw4l-sm64-native: step %u decomp update complete\n", bridge_step);
                fflush(stderr);
            }

            /* Serialize the decomp's resulting Mario state before the next
               external-player write. This is how cannon launches, moving
               platforms, warps and knockback are handed back to COD. */
            gBridgeStage = "write_snapshot";
            write_snapshot();

            if (bridge_step <= 3 || (bridge_step % 30u) == 0u) {
                fprintf(stderr, "iw4l-sm64-native: step %u snapshot complete\n", bridge_step);
                fflush(stderr);
            }
            gBridgeStage = "waiting_request";
        }
    }

    gBridgeStage = "shutdown";
    return 0;
}

#if defined(_WIN32) || defined(_WIN64)
extern int __argc;
extern char **__argv;
int WINAPI WinMain(
    UNUSED HINSTANCE hInstance,
    UNUSED HINSTANCE hPrevInstance,
    UNUSED LPSTR pCmdLine,
    UNUSED int nCmdShow
) {
    return bridge_main(__argc, __argv);
}
#else
int main(int argc, char **argv) {
    return bridge_main(argc, argv);
}
#endif
#endif /* !IW4L_SM64_EMBEDDED */
