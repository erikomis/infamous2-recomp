/*
 * A heartbeat on the controller while Cole is badly hurt.
 *
 * inFAMOUS 2 shows health without a bar: the picture loses its colour and a
 * heartbeat plays. The controller stays still (the game only runs the motors
 * in short bursts for jumps, landings and hits; PS3_RUMBLE_LOG=1 shows them).
 * Here the port reads Cole's health from guest memory and, below a threshold,
 * beats a lub-dub on the motors through the toolkit's cellPad_host_pulse,
 * which adds it to the game's own rumble (the stronger of the two per motor)
 * and lets the DualSense light bar glow red with it. Faster and stronger as
 * health runs out: 70 beats a minute at the threshold, 140 at nothing left.
 *
 * Where health lives (notes/health/README.md has how it was found):
 *
 *   0x00A6E9E4  u32   the hero object (vtable 0x0093E1E0), 0 = none
 *     +0x7EC    f32   maximum hit points   (script: MAX_HIT_POINTS)
 *     +0x7F0    f32   current hit points   (script: CURRENT_HIT_POINTS);
 *                     -1 once he is dead, until the object is replaced
 *
 * The object is a new one after every death, at another address: the pointer
 * is read again on every tick. The game's own picture follows the same
 * number (colour fading from about 42% of the maximum, black and white by
 * 33%), and its own recovery starts ten seconds after the last hit.
 *
 * It is silent whenever the game hides its own HUD, by the test the HUD
 * widgets flagged HIDE_ON_LETTERBOX use (func_00465580): a letterbox or
 * HUD-hide request is up (the counters at 0xA73130+0x648/+0x650, second
 * viewport +0xCD8/+0xCE0), the world clock is stopped (scale 0 at 0xA2C510:
 * the pause menu, a loading screen), or the pause menu is open (count at
 * 0xA76318). So: nothing in cutscenes, while loading, in the pause menu, on
 * death, or when frames stop coming.
 *
 *   PS3_HEARTBEAT=0             off (on by default)
 *   PS3_HEARTBEAT_AT=<5-90>     the health, in percent, it starts below (35)
 *   PS3_HEARTBEAT_STRENGTH=<0-200>  percent of the beat's strength (100); the
 *                               vibration strength (PS3_RUMBLE_STRENGTH)
 *                               applies on top, and PS3_RUMBLE=0 silences it
 *   PS3_HEARTBEAT_LOG=1         print health as it changes, and each beat;
 *                               =2 adds the raw words behind the state
 *
 * The pattern is a pure function of health and time (hb_level/hb_step),
 * tested in tests/test_heartbeat.c.
 */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#ifndef _WIN32
#include <pthread.h>
#include <unistd.h>
#endif

extern uint8_t* vm_base;
/* Toolkit, libs/input/cellPad.c: a host pulse on top of the title's rumble. */
void cellPad_host_pulse(int port, unsigned lo, unsigned hi);

/* ---- the game's memory --------------------------------------------------- */
#define HB_HERO_PTR_EA     0x00A6E9E4u
#define HB_HERO_VTABLE     0x0093E1E0u
#define HB_HERO_MAX_HP     0x07ECu
#define HB_HERO_HP         0x07F0u
#define HB_HUD_EA          0x00A73130u   /* per viewport: +0x648/+0x650, next at +0x690 */
#define HB_HUD_FORCE_EA    0x00A76314u   /* non-zero: the HUD ignores the letterbox */
#define HB_PAUSE_COUNT_EA  0x00A76318u   /* pause menus open */
#define HB_CLOCK_SCALE_EA  0x00A2C510u   /* the world clock's scale (game_clock.c) */

static uint32_t hb_rd32(uint32_t ea)
{
    const uint8_t* p = vm_base + ea;
    return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3];
}
static float hb_rdf(uint32_t ea) { uint32_t u = hb_rd32(ea); float f; memcpy(&f, &u, 4); return f; }

/* What the game's state says, read in one go. */
typedef struct {
    uint32_t hero;        /* 0 = no hero object */
    float hp, max_hp;
    int hud_hidden;       /* letterbox / HUD hidden / world clock stopped / pause menu */
    /* the words behind it (PS3_HEARTBEAT_LOG=2) */
    int32_t letterbox, hud_hide, force, pause;
    float scale;
} HbGame;

static void hb_read(HbGame* g)
{
    memset(g, 0, sizeof *g);
    const uint32_t hero = hb_rd32(HB_HERO_PTR_EA);
    /* The game tests this pointer for null and nothing else; the range keeps
     * the read inside guest memory whatever the word holds, and the vtable
     * check keeps a stale object, or another build of the game, from being
     * read as Cole. */
    if (hero >= 0x00010000u && hero < 0xD0000000u && !(hero & 3u) && hb_rd32(hero) == HB_HERO_VTABLE) {
        g->hero = hero;
        g->max_hp = hb_rdf(hero + HB_HERO_MAX_HP);
        g->hp = hb_rdf(hero + HB_HERO_HP);
    }
    g->letterbox = (int32_t)hb_rd32(HB_HUD_EA + 0x648u) + (int32_t)hb_rd32(HB_HUD_EA + 0xCD8u);
    g->hud_hide  = (int32_t)hb_rd32(HB_HUD_EA + 0x650u) + (int32_t)hb_rd32(HB_HUD_EA + 0xCE0u);
    g->scale = hb_rdf(HB_CLOCK_SCALE_EA);
    g->force = (int32_t)hb_rd32(HB_HUD_FORCE_EA);
    g->pause = (int32_t)hb_rd32(HB_PAUSE_COUNT_EA);
    const int boxed = g->letterbox > 0 || g->hud_hide > 0 || !(fabsf(g->scale) >= 1e-4f);
    g->hud_hidden = (boxed && g->force == 0) || g->pause > 0;
}

/* Health as a fraction of the maximum, or -1 when there is nothing to beat
 * for: no hero, dead (no hit points left), the HUD hidden, or numbers that
 * are not health. */
static float hb_fraction(const HbGame* g)
{
    if (!g->hero || g->hud_hidden) return -1.0f;
    if (!(g->max_hp >= 1.0f) || !(g->max_hp < 1e6f) || !(g->hp > 0.0f)) return -1.0f;
    const float f = g->hp / g->max_hp;
    return f > 1.0f ? 1.0f : f;
}

/* ---- the pattern --------------------------------------------------------- */
#define HB_BPM_MIN     70.0f    /* at the threshold */
#define HB_BPM_MAX    140.0f    /* at no health left */
#define HB_AMP_MIN      0.45f   /* the first beat's share of the motor, same ends */
#define HB_AMP_MAX      1.00f
#define HB_LUB_S        0.090f  /* the first beat lasts this long */
#define HB_DUB_S        0.070f  /* the second one */
#define HB_DUB_AT_S     0.300f  /* it starts this long after the first, or at ... */
#define HB_DUB_AT_BEAT  0.36f   /* ... this part of the beat, whichever is sooner */
#define HB_DUB_GAIN     0.60f   /* the second beat is softer */
#define HB_LUB_SMALL    0.35f   /* the small motor's share of the first beat (a sharper edge) */
#define HB_HYSTERESIS   0.03f   /* it stops this far above the threshold */
#define HB_ARM_S        0.15f   /* health has to be readable this long before it starts:
                                   the HUD shows for a frame or two between a load and
                                   its cutscene */

typedef struct { unsigned lo, hi; } HbLevel;   /* large and small motor, 0-65535 */
typedef struct { int on; float t, seen; } HbState;   /* beating; seconds into the beat; seconds of health in sight */

/* 0 at the threshold, 1 with no health left. */
static float hb_urgency(float frac, float thr)
{
    const float u = 1.0f - frac / thr;
    return u < 0.0f ? 0.0f : u > 1.0f ? 1.0f : u;
}
static float hb_bpm(float u) { return HB_BPM_MIN + (HB_BPM_MAX - HB_BPM_MIN) * u; }
static float hb_period(float u) { return 60.0f / hb_bpm(u); }

/* The motors t seconds into a beat. */
static HbLevel hb_level(float u, float t, int strength_pct)
{
    HbLevel l = { 0, 0 };
    const float period = hb_period(u);
    float dub_at = HB_DUB_AT_BEAT * period;
    if (dub_at > HB_DUB_AT_S) dub_at = HB_DUB_AT_S;
    float amp = (HB_AMP_MIN + (HB_AMP_MAX - HB_AMP_MIN) * u) * (float)strength_pct / 100.0f;
    if (amp > 1.0f) amp = 1.0f;
    if (!(amp > 0.0f) || t < 0.0f) return l;
    if (t < HB_LUB_S) {
        l.lo = (unsigned)(amp * 65535.0f);
        l.hi = (unsigned)(amp * HB_LUB_SMALL * 65535.0f);
    } else if (t >= dub_at && t < dub_at + HB_DUB_S) {
        l.lo = (unsigned)(amp * HB_DUB_GAIN * 65535.0f);
    }
    return l;
}

/* dt seconds later, with health at frac of the maximum (negative: nothing to
 * beat for). Starts on a first beat below thr, once health has been in sight
 * for HB_ARM_S; stops at once on a negative frac, and once health is
 * HB_HYSTERESIS above thr. */
static HbLevel hb_step(HbState* s, float dt, float frac, float thr, int strength_pct)
{
    const HbLevel none = { 0, 0 };
    if (!(frac > 0.0f)) { s->on = 0; s->t = 0.0f; s->seen = 0.0f; return none; }
    const float u = hb_urgency(frac, thr);
    if (s->seen < HB_ARM_S) s->seen += dt > 0.0f ? dt : 0.0f;
    if (!s->on) {
        if (frac >= thr || s->seen < HB_ARM_S) return none;
        s->on = 1;
        s->t = 0.0f;
    } else {
        if (frac >= thr + HB_HYSTERESIS) { s->on = 0; s->t = 0.0f; return none; }
        const float period = hb_period(u);
        s->t += dt > 0.0f ? dt : 0.0f;
        if (s->t >= period) { s->t -= period; if (s->t >= period) s->t = 0.0f; }
    }
    return hb_level(u, s->t, strength_pct);
}

/* ---- the port's side ----------------------------------------------------- */
#define HB_TICK_US         8000     /* the beat's time resolution */
#define HB_FRAME_STALE_S   1.0      /* no frame for this long (a hang, not a hitch): silent */

static int hb_env_int(const char* name, int def, int lo, int hi)
{
    const char* e = getenv(name);
    if (!e || !*e) return def;
    char* end = NULL;
    const long v = strtol(e, &end, 10);
    if (end == e) return def;
    return v < lo ? lo : v > hi ? hi : (int)v;
}

#ifndef HEARTBEAT_TEST
static double hb_now(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec + (double)t.tv_nsec * 1e-9;
}

static double s_hb_t0;               /* process start: log times are PAD_SCRIPT's */
#if defined(__GNUC__) || defined(__clang__)
__attribute__((constructor)) static void hb_t0(void) { s_hb_t0 = hb_now(); }
#endif
static volatile double s_hb_frame;   /* the last frame (hb_now) */
static float s_hb_thr;               /* threshold, a fraction */
static int s_hb_strength, s_hb_log;

static void* hb_thread(void* arg)
{
    (void)arg;
    HbState st = { 0, 0.0f, 0.0f };
    HbGame last; memset(&last, 0, sizeof last);
    int was_on = 0, was_level = 0, beat_part = 0;
    double prev = hb_now(), log_at = 0.0;
    float log_hp = -1.0f;
    for (;;) {
        usleep(HB_TICK_US);
        const double now = hb_now();
        const float dt = (float)(now - prev);
        prev = now;
        HbGame g; memset(&g, 0, sizeof g);
        const int frames = vm_base && now - s_hb_frame < HB_FRAME_STALE_S;
        if (frames) hb_read(&g);
        const float frac = frames ? hb_fraction(&g) : -1.0f;
        const HbLevel l = hb_step(&st, dt, frac, s_hb_thr, s_hb_strength);

        const int level = l.lo || l.hi;
        if (level || was_level) cellPad_host_pulse(0, l.lo, l.hi);
        was_level = level;

        const double t = now - s_hb_t0;
        if (st.on != was_on) {
            if (st.on)
                fprintf(stderr, "[heartbeat] t=%.2f on: health %.0f%% (%.1f/%.1f), %.0f bpm\n",
                        t, frac * 100.0f, g.hp, g.max_hp, hb_bpm(hb_urgency(frac, s_hb_thr)));
            else
                fprintf(stderr, "[heartbeat] t=%.2f off: %s (%.1f/%.1f)\n", t,
                        !frames ? "no frames" : !g.hero ? "no hero" : !(g.hp > 0.0f) ? "dead" :
                        g.hud_hidden ? "cutscene or menu" : "health back", g.hp, g.max_hp);
            was_on = st.on;
        }
        if (s_hb_log && frames) {
            /* Health when it moves (at most 5 lines a second), the state when it changes. */
            int state = g.hero != last.hero || g.hud_hidden != last.hud_hidden || g.max_hp != last.max_hp;
            if (s_hb_log > 1)
                state |= g.letterbox != last.letterbox || g.hud_hide != last.hud_hide ||
                         g.force != last.force || g.pause != last.pause || (g.scale != last.scale && now - log_at >= 0.2);
            if (state || (g.hp != log_hp && now - log_at >= 0.2)) {
                fprintf(stderr, "[heartbeat] t=%.2f health %.1f/%.1f (%.0f%%) hero=%08X%s%s", t, g.hp, g.max_hp,
                        g.max_hp > 0.0f ? g.hp / g.max_hp * 100.0f : 0.0f, g.hero,
                        g.hud_hidden ? " hud-hidden" : "", st.on ? " beating" : "");
                if (s_hb_log > 1)
                    fprintf(stderr, " | letterbox=%d hudhide=%d scale=%g force=%d pause=%d",
                            g.letterbox, g.hud_hide, (double)g.scale, g.force, g.pause);
                fputc('\n', stderr);
                log_hp = g.hp; log_at = now; last = g;
            }
            const int part = !level ? 0 : l.hi ? 1 : 2;
            if (part && part != beat_part)
                fprintf(stderr, "[heartbeat] t=%.3f %s lo=%u hi=%u (%.0f bpm)\n", t, part == 1 ? "lub" : "dub",
                        l.lo, l.hi, hb_bpm(hb_urgency(frac, s_hb_thr)));
            beat_part = part;
        }
    }
    return NULL;
}

#ifdef HEARTBEAT_POKE
/* A build made for checking the reading, never the shipped one (CMake
 * -DPORT_HEARTBEAT_POKE=ON): PS3_HEARTBEAT_POKE=<sec>:<hp>[,<sec>:<hp>...]
 * writes Cole's hit points at those times, in order (seconds since the
 * process started; w<sec> counts from the moment the file named by
 * PAD_SCRIPT_WAIT appears, like the toolkit's PAD_SCRIPT), and marks him as
 * just hit in the game's recovery state machine (hero +0x1F14 = 1), as a
 * real hit does. The game takes it from there: the picture loses its colour,
 * the blood comes up, and ten seconds later its own recovery brings the
 * health back. notes/health/README.md shows a run. */
#define HB_HERO_REGEN_STATE 0x1F14u   /* 0 idle, 1 just hit, 2 waiting 10 s, 3 recovering */
static void hb_poke(double t)
{
    static const char* next = (const char*)-1;
    static double mark = -1.0;
    if (next == (const char*)-1) next = getenv("PS3_HEARTBEAT_POKE");
    if (!next || !*next) return;
    const int rel = *next == 'w';
    if (rel && mark < 0.0) {
        const char* w = getenv("PAD_SCRIPT_WAIT");
        FILE* f = w && *w ? fopen(w, "rb") : NULL;
        if (!f) return;
        fclose(f);
        mark = t;
    }
    char* end = NULL;
    const double at = strtod(next + rel, &end) + (rel ? mark : 0.0);
    if (end == next + rel || *end != ':') { next = NULL; return; }
    if (t < at) return;
    const float hp = strtof(end + 1, &end);
    next = *end == ',' ? end + 1 : NULL;
    HbGame g;
    hb_read(&g);
    if (!g.hero || !(g.hp > 0.0f)) { fprintf(stderr, "[heartbeat] t=%.2f poke skipped: no living hero\n", t); return; }
    uint32_t u; memcpy(&u, &hp, 4);
    uint8_t* p = vm_base + g.hero + HB_HERO_HP;
    p[0] = (uint8_t)(u >> 24); p[1] = (uint8_t)(u >> 16); p[2] = (uint8_t)(u >> 8); p[3] = (uint8_t)u;
    p = vm_base + g.hero + HB_HERO_REGEN_STATE;
    p[0] = p[1] = p[2] = 0; p[3] = 1;
    fprintf(stderr, "[heartbeat] t=%.2f poke: hit points %.1f -> %.1f\n", t, g.hp, hp);
}
#endif

/* Once per presented frame (main.cpp). The first call reads the knobs and
 * starts the beat's thread; later ones only say that frames are coming. */
void infamous2_heartbeat_frame(void)
{
    static int state;   /* 0 not started, 1 running, -1 off */
#ifdef HEARTBEAT_POKE
    if (vm_base && s_hb_t0 != 0.0) hb_poke(hb_now() - s_hb_t0);
#endif
    if (state < 0) return;
    const double now = hb_now();
    if (state == 0) {
        const char* e = getenv("PS3_HEARTBEAT");
        if (e && e[0] == '0' && e[1] == 0) { state = -1; return; }
        s_hb_thr = (float)hb_env_int("PS3_HEARTBEAT_AT", 35, 5, 90) / 100.0f;
        s_hb_strength = hb_env_int("PS3_HEARTBEAT_STRENGTH", 100, 0, 200);
        s_hb_log = hb_env_int("PS3_HEARTBEAT_LOG", 0, 0, 2);
        if (s_hb_t0 == 0.0) s_hb_t0 = now;
        s_hb_frame = now;
        pthread_t th;
        if (!s_hb_strength || pthread_create(&th, NULL, hb_thread, NULL) != 0) { state = -1; return; }
        pthread_detach(th);
        state = 1;
        return;
    }
    s_hb_frame = now;
}
#endif /* HEARTBEAT_TEST */
