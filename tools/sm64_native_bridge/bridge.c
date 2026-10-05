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
#include "levels/scripts.h"
#include "game/area.h"
#include "game/game_init.h"
#include "game/interaction.h"
#include "game/level_update.h"
#include "game/mario.h"
#include "game/memory.h"
#include "game/object_list_processor.h"
#include "game/platform_displacement.h"
#include "game/save_file.h"
#include "audio/external.h"
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
static volatile const char *gBridgeStage = "startup";


OSMesg gMainReceivedMesg;
OSMesgQueue gSIEventMesgQueue;

s8 gResetTimer;
s8 gNmiResetBarsTimer;
s8 gDebugLevelSelect;
s8 gShowProfiler;
s8 gShowDebugText;

extern void thread5_game_loop(void *arg);

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

static void apply_proxy(const struct Request *request) {
    if (gMarioState == NULL || gMarioObject == NULL) {
        return;
    }

    /*
     * COD is authoritative for ordinary locomotion. The decomp is authoritative
     * for object/automatic/cutscene actions such as cannons, poles, warps and
     * grabs. Do not allow stale walking/airborne state from a previous native
     * frame to survive after COD teleports the hidden Mario proxy.
     */
    switch (gMarioState->action & ACT_GROUP_MASK) {
        case ACT_GROUP_OBJECT:
        case ACT_GROUP_AUTOMATIC:
        case ACT_GROUP_CUTSCENE:
            break;
        default:
            if (gMarioState->action != ACT_IDLE) {
                set_mario_action(gMarioState, ACT_IDLE, 0);
            }
            break;
    }

    /*
     * update_objects() applies last frame's gMarioPlatform displacement before
     * Mario's action refreshes collision. Since COD replaces Mario's position
     * every bridge tick, that platform reference is stale by definition and
     * can point at an object state unrelated to the new external position.
     */
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

    /*
     * MarioState caches collision pointers/heights. Because COD can move the
     * proxy arbitrarily between native ticks, those caches must be rebuilt at
     * the new position before any vanilla Mario/object behavior runs. Keeping
     * a floor/ceiling pointer from the previous external position can leave a
     * dangling dynamic-surface reference and eventually crash update_level().
     */
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

    if (gPlayer1Controller != NULL) {
        u16 buttons = 0;

        /* Primary fire becomes the cannon's A press only while Mario's hidden
           proxy is actually in the cannon action. It must not turn ordinary
           COD gunfire into Mario jumps. */
        if (gMarioState->action == ACT_IN_CANNON && (request->attack_flags & 1u)) {
            buttons |= A_BUTTON;
        }

        gPlayer1Controller->buttonDown = buttons;
        gPlayer1Controller->buttonPressed = buttons;
        gPlayer1Controller->rawStickX = 0;
        gPlayer1Controller->rawStickY = 0;
        gPlayer1Controller->stickX = 0.0f;
        gPlayer1Controller->stickY = 0.0f;
        gPlayer1Controller->stickMag = 0.0f;
    }

    if (gMarioState->action == ACT_IN_CANNON && gMarioState->usedObj != NULL) {
        s32 inputYaw = (s16)(request->yaw - gMarioState->usedObj->oMoveAngleYaw);
        if (inputYaw > 0x4000) inputYaw = 0x4000;
        if (inputYaw < -0x4000) inputYaw = -0x4000;
        gMarioObject->oMarioCannonInputYaw = inputYaw;
        gMarioState->faceAngle[0] = request->pitch;
    }
}

static void apply_external_attack(const struct Request *request) {
    if ((request->attack_flags & 1u) == 0 || gObjectLists == NULL) {
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
                if (object == gMarioObject || (object->activeFlags & ACTIVE_FLAG_ACTIVE) == 0) {
                    continue;
                }
                if (object->oInteractType == 0) {
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
                if (object == gMarioObject || (object->activeFlags & ACTIVE_FLAG_ACTIVE) == 0) {
                    continue;
                }
                if (object->oInteractType == 0) {
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
                            "iw4l-sm64-bridge: warning: dynamic surface chain guard tripped at cell=(%d,%d) partition=%d\n",
                            x, z, partition
                        );
                        break;
                    }

                    surface=node->surface;
                    if (surface == NULL) {
                        fprintf(
                            stderr,
                            "iw4l-sm64-bridge: warning: null dynamic surface at cell=(%d,%d) partition=%d\n",
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
                            "iw4l-sm64-bridge: warning: dynamic surface pointer outside pool at cell=(%d,%d) partition=%d\n",
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
        "iw4l-sm64-bridge: census objects=%u trees=%u yellow_coins=%u red_coins=%u cannons=%u stars=%u goombas=%u dynamic_surfaces=%u\n",
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

static void write_snapshot(void) {
    const struct Object *objects[4096];
    const struct Surface *dynamic_surfaces[8192];
    uint32_t count;
    uint32_t dynamic_count;
    uint32_t i;
    static uint32_t snapshot_number = 0;

    snapshot_number++;

    if (snapshot_number <= 3 || (snapshot_number % 30u) == 0u) {
        fprintf(stderr, "iw4l-sm64-bridge: snapshot %u collecting objects\n", snapshot_number);
        fflush(stderr);
    }
    gBridgeStage = "collect_objects";
    count = collect_objects(objects, 4096);

    if (snapshot_number <= 3 || (snapshot_number % 30u) == 0u) {
        fprintf(stderr, "iw4l-sm64-bridge: snapshot %u objects=%u; collecting dynamic surfaces\n",
                snapshot_number, count);
        fflush(stderr);
    }
    gBridgeStage = "collect_dynamic_surfaces";
    dynamic_count = collect_dynamic_surfaces(dynamic_surfaces, 8192);

    if (snapshot_number <= 3 || (snapshot_number % 30u) == 0u) {
        fprintf(stderr, "iw4l-sm64-bridge: snapshot %u dynamic_surfaces=%u\n",
                snapshot_number, dynamic_count);
        fflush(stderr);
    }

    print_native_census_once(objects, count, dynamic_count);

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
    write_u16(0);

    for (i = 0; i < count; ++i) {
        const struct Object *object = objects[i];
        write_u32(object_id(object));
        write_i32(model_id_for_object(object));

        /*
         * Use gameplay-space oPos for the presentation root. This is the
         * coordinate that matches level-script/macro-object placement and
         * collision. header.gfx.pos may contain behavior-specific visual
         * offsets and is therefore not a stable world-space anchor.
         */
        write_f32(object->oPosX);
        write_f32(object->oPosY);
        write_f32(object->oPosZ);
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
        "iw4l-sm64-bridge: FATAL native exception code=0x%08lX address=%p globalTimer=%u stage=%s\n",
        (unsigned long)code,
        address,
        (unsigned)gGlobalTimer,
        gBridgeStage != NULL ? (const char *)gBridgeStage : "unknown"
    );
    fflush(stderr);
    return EXCEPTION_EXECUTE_HANDLER;
}
#endif

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
        fprintf(stderr, "iw4l-sm64-bridge: unknown level '%s'\n", level_name);
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

    /* The COD mashup has no Mario dialogue UI yet, so Bob-omb Buddy cannot
       perform the vanilla cannon-unlock conversation. Unlock the course
       cannon flag before the level initializes; the original bhvCannonClosed,
       bhvCannon and ACT_IN_CANNON code still own the actual cannon behavior. */
    save_file_set_cannon_unlocked();

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
                "iw4l-sm64-bridge: level '%s' did not initialize area/player (area=%d act=%d)\n",
                level_name, area, act);
        return 3;
    }

    if ((gMarioState->action & ACT_GROUP_MASK) == ACT_GROUP_CUTSCENE) {
        fprintf(
            stderr,
            "iw4l-sm64-bridge: normalizing unexpected startup cutscene action 0x%08X to ACT_IDLE\n",
            gMarioState->action
        );
        set_mario_action(gMarioState, ACT_IDLE, 0);
    }

    fprintf(stderr,
            "iw4l-sm64-bridge: native decomp gameplay ready: level=%s area=%d act=%d action=0x%08X\n",
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
                fprintf(stderr, "iw4l-sm64-bridge: step %u begin\n", bridge_step);
                fflush(stderr);
            }

            gBridgeStage = "apply_proxy";
            apply_proxy(&request);
            gBridgeStage = "apply_external_attack";
            apply_external_attack(&request);

            if (bridge_step <= 3 || (bridge_step % 30u) == 0u) {
                fprintf(stderr, "iw4l-sm64-bridge: step %u proxy applied\n", bridge_step);
                fflush(stderr);
            }

            gBridgeStage = "select_gfx_pool";
            select_gfx_pool();
            gBridgeStage = "level_script_execute";
            level_command = level_script_execute(level_command);
            gGlobalTimer++;

            if (bridge_step <= 3 || (bridge_step % 30u) == 0u) {
                fprintf(stderr, "iw4l-sm64-bridge: step %u decomp update complete\n", bridge_step);
                fflush(stderr);
            }

            /* Serialize the decomp's resulting Mario state before the next
               external-player write. This is how cannon launches, moving
               platforms, warps and knockback are handed back to COD. */
            gBridgeStage = "write_snapshot";
            write_snapshot();

            if (bridge_step <= 3 || (bridge_step % 30u) == 0u) {
                fprintf(stderr, "iw4l-sm64-bridge: step %u snapshot complete\n", bridge_step);
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
