/* heartbeat.c: the lub-dub pattern and the reading of the game's state.
 *
 *   cc -std=gnu17 -Wall -Wextra -DHEARTBEAT_TEST infamous2-lle/tests/test_heartbeat.c \
 *      -o /tmp/test_heartbeat && /tmp/test_heartbeat
 *
 * Nothing of the game or the toolkit is linked: the pattern is a pure
 * function, and the guest memory is a zeroed buffer the test writes into.
 */
#include "../heartbeat.c"
#include <assert.h>
#include <sys/mman.h>

uint8_t* vm_base;
void cellPad_host_pulse(int port, unsigned lo, unsigned hi) { (void)port; (void)lo; (void)hi; }

static int near(float a, float b, float eps) { return fabsf(a - b) <= eps; }

static void test_rate_and_strength(void)
{
    const float thr = 0.35f;
    assert(hb_urgency(0.35f, thr) == 0.0f && hb_urgency(0.90f, thr) == 0.0f);
    assert(hb_urgency(0.0f, thr) == 1.0f);
    assert(near(hb_urgency(0.175f, thr), 0.5f, 1e-5f));
    assert(hb_bpm(0.0f) == 70.0f && hb_bpm(1.0f) == 140.0f && near(hb_bpm(0.5f), 105.0f, 1e-3f));
    assert(near(hb_period(0.0f), 0.857f, 1e-3f) && near(hb_period(1.0f), 0.4286f, 1e-3f));

    /* At the threshold: the first beat at 45% on the large motor with a touch
     * of the small one, the second 0.3 s later at 60% of it, silence between
     * and after. */
    HbLevel l = hb_level(0.0f, 0.0f, 100);
    assert(l.lo == (unsigned)(0.45f * 65535.0f) && l.hi == (unsigned)(0.45f * 0.35f * 65535.0f));
    l = hb_level(0.0f, 0.089f, 100); assert(l.lo && l.hi);
    l = hb_level(0.0f, 0.091f, 100); assert(!l.lo && !l.hi);
    l = hb_level(0.0f, 0.299f, 100); assert(!l.lo && !l.hi);
    l = hb_level(0.0f, 0.301f, 100);
    assert(l.lo == (unsigned)(0.45f * 0.60f * 65535.0f) && l.hi == 0);          /* softer, large motor only */
    l = hb_level(0.0f, 0.369f, 100); assert(l.lo && !l.hi);
    l = hb_level(0.0f, 0.371f, 100); assert(!l.lo && !l.hi);
    l = hb_level(0.0f, 0.80f, 100);  assert(!l.lo && !l.hi);

    /* With no health left: full strength, and the second beat moves up so
     * both fit a 0.43 s beat with a rest after them. */
    l = hb_level(1.0f, 0.0f, 100);
    assert(l.lo == 65535 && l.hi == (unsigned)(0.35f * 65535.0f));
    l = hb_level(1.0f, 0.12f, 100);  assert(!l.lo && !l.hi);                    /* the gap between the two */
    l = hb_level(1.0f, 0.16f, 100);  assert(l.lo == (unsigned)(0.60f * 65535.0f) && !l.hi);
    l = hb_level(1.0f, 0.23f, 100);  assert(!l.lo && !l.hi);                    /* the rest */
    l = hb_level(1.0f, 0.42f, 100);  assert(!l.lo && !l.hi);
    /* Stronger as health falls, in between. */
    assert(hb_level(0.5f, 0.0f, 100).lo > hb_level(0.0f, 0.0f, 100).lo);
    assert(hb_level(1.0f, 0.0f, 100).lo > hb_level(0.5f, 0.0f, 100).lo);

    /* PS3_HEARTBEAT_STRENGTH: scaled, capped at the motor's maximum, 0 = nothing. */
    assert(hb_level(0.0f, 0.0f, 50).lo == (unsigned)(0.225f * 65535.0f));
    assert(hb_level(0.0f, 0.0f, 200).lo == (unsigned)(0.90f * 65535.0f));
    assert(hb_level(1.0f, 0.0f, 200).lo == 65535 && hb_level(1.0f, 0.0f, 200).hi == (unsigned)(0.35f * 65535.0f));
    l = hb_level(1.0f, 0.0f, 0); assert(!l.lo && !l.hi);
}

/* Run the state machine for `secs` at health `frac`; count beats (rising
 * edges of the first beat) and what the motors did. */
typedef struct { int lubs, dubs; unsigned max_lo; int active_ticks, ticks; } Run;
static Run run(HbState* s, float frac, float secs)
{
    Run r; memset(&r, 0, sizeof r);
    const float dt = 0.008f;
    int prev = 0;
    for (float t = 0.0f; t < secs; t += dt) {
        const HbLevel l = hb_step(s, dt, frac, 0.35f, 100);
        const int part = !l.lo ? 0 : l.hi ? 1 : 2;
        if (part == 1 && prev != 1) r.lubs++;
        if (part == 2 && prev != 2) r.dubs++;
        if (l.lo > r.max_lo) r.max_lo = l.lo;
        if (part) r.active_ticks++;
        r.ticks++;
        prev = part;
    }
    return r;
}

/* Not beating, health in sight for long enough to start (HB_ARM_S). */
static void reset(HbState* s) { s->on = 0; s->t = 0.0f; s->seen = 1.0f; }

static void test_state_machine(void)
{
    HbState s = { 0, 0.0f, 0.0f };
    /* Healthy: nothing, however long. */
    Run r = run(&s, 1.0f, 5.0f);
    assert(!s.on && r.lubs == 0 && r.dubs == 0 && r.max_lo == 0);
    r = run(&s, 0.35f, 5.0f);                      /* exactly at the threshold: not yet */
    assert(!s.on && r.lubs == 0);

    /* Just below: it starts on a first beat at once, about 70 a minute. */
    HbLevel l = hb_step(&s, 0.008f, 0.349f, 0.35f, 100);
    assert(s.on && l.lo && l.hi);
    reset(&s);
    r = run(&s, 0.349f, 60.0f);
    assert(s.on && r.lubs >= 69 && r.lubs <= 72 && r.dubs >= r.lubs - 1 && r.dubs <= r.lubs);
    assert(r.active_ticks * 5 < r.ticks);         /* mostly silence: beats, not a buzz */
    /* Half way down: about 105; nearly dead: about 140, and stronger. */
    reset(&s);
    Run mid = run(&s, 0.175f, 60.0f);
    assert(mid.lubs >= 103 && mid.lubs <= 107);
    reset(&s);
    Run low = run(&s, 0.004f, 60.0f);
    assert(low.lubs >= 136 && low.lubs <= 141 && low.dubs >= low.lubs - 1);
    assert(low.max_lo > mid.max_lo && mid.max_lo > r.max_lo);

    /* Health coming back: it keeps beating up to the threshold and a little
     * past it (no chatter around the line), then stops. */
    r = run(&s, 0.36f, 3.0f);
    assert(s.on && r.lubs >= 3);
    l = hb_step(&s, 0.008f, 0.381f, 0.35f, 100);
    assert(!s.on && !l.lo && !l.hi);
    r = run(&s, 0.36f, 3.0f);                      /* above the line: it does not start again */
    assert(!s.on && r.lubs == 0);

    /* Death, a cutscene, a menu (a negative fraction): silent on that very
     * step, even in the middle of a beat, and for as long as it lasts. */
    l = hb_step(&s, 0.008f, 0.10f, 0.35f, 100);
    assert(s.on && l.lo);
    l = hb_step(&s, 0.008f, -1.0f, 0.35f, 100);
    assert(!s.on && !l.lo && !l.hi);
    r = run(&s, -1.0f, 5.0f);
    assert(r.lubs == 0 && r.max_lo == 0);
    l = hb_step(&s, 0.008f, 0.0f, 0.35f, 100);     /* no health at all is dead too */
    assert(!s.on && !l.lo);
    l = hb_step(&s, 0.008f, NAN, 0.35f, 100);
    assert(!s.on && !l.lo);
    /* Back in play still hurt: it starts again on a first beat, but only
     * once health has been in sight for 0.15 s -- the HUD shows for a frame
     * or two between a load and its cutscene, and that must not tick. */
    l = hb_step(&s, 0.008f, 0.10f, 0.35f, 100);
    assert(!s.on && !l.lo && !l.hi);
    for (int i = 0; i < 6; i++) l = hb_step(&s, 0.008f, 0.10f, 0.35f, 100);     /* 56 ms in sight */
    assert(!s.on && !l.lo);
    l = hb_step(&s, 0.008f, -1.0f, 0.35f, 100);                                  /* gone again */
    for (int i = 0; i < 17; i++) l = hb_step(&s, 0.008f, 0.10f, 0.35f, 100);    /* 136 ms: counted from scratch */
    assert(!s.on && !l.lo);
    int more = 0;
    while (!s.on && more < 10) { l = hb_step(&s, 0.008f, 0.10f, 0.35f, 100); more++; }
    assert(s.on && more <= 3 && l.lo && l.hi && s.t == 0.0f);                    /* at about 150 ms */

    /* A stalled thread (one long step) does not burst: at most one beat. */
    l = hb_step(&s, 30.0f, 0.10f, 0.35f, 100);
    assert(s.on && s.t == 0.0f);
    (void)l;
}

static void wr32(uint32_t ea, uint32_t v)
{
    vm_base[ea] = (uint8_t)(v >> 24); vm_base[ea + 1] = (uint8_t)(v >> 16);
    vm_base[ea + 2] = (uint8_t)(v >> 8); vm_base[ea + 3] = (uint8_t)v;
}
static void wrf(uint32_t ea, float f) { uint32_t u; memcpy(&u, &f, 4); wr32(ea, u); }

static void test_game_state(void)
{
    const uint32_t size = 0x01000000u, hero = 0x00C00000u;
    vm_base = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANON, -1, 0);
    assert(vm_base != MAP_FAILED);
    HbGame g;

    /* Boot: no hero, clock not running. */
    hb_read(&g);
    assert(!g.hero && hb_fraction(&g) < 0.0f);

    /* Cole in play at 124 of 400. */
    wr32(HB_HERO_PTR_EA, hero);
    wr32(hero, HB_HERO_VTABLE);
    wrf(hero + HB_HERO_MAX_HP, 400.0f);
    wrf(hero + HB_HERO_HP, 124.0f);
    wrf(HB_CLOCK_SCALE_EA, 1.0f);
    hb_read(&g);
    assert(g.hero == hero && g.hp == 124.0f && g.max_hp == 400.0f && !g.hud_hidden);
    assert(near(hb_fraction(&g), 0.31f, 1e-6f));

    /* A letterbox (a cutscene), on either viewport; a HUD-hide request. */
    wr32(HB_HUD_EA + 0x648u, 1); hb_read(&g); assert(g.hud_hidden && hb_fraction(&g) < 0.0f);
    wr32(HB_HUD_EA + 0x648u, 0); wr32(HB_HUD_EA + 0xCD8u, 2); hb_read(&g); assert(g.hud_hidden);
    wr32(HB_HUD_EA + 0xCD8u, 0); wr32(HB_HUD_EA + 0x650u, 1); hb_read(&g); assert(g.hud_hidden);
    wr32(HB_HUD_FORCE_EA, 1); hb_read(&g); assert(!g.hud_hidden);      /* the game's own override */
    wr32(HB_HUD_FORCE_EA, 0);
    wr32(HB_HUD_EA + 0x650u, 0); hb_read(&g); assert(!g.hud_hidden);
    /* The world clock stopped (pause, loading); the pause menu's own count. */
    wrf(HB_CLOCK_SCALE_EA, 0.0f); hb_read(&g); assert(g.hud_hidden && hb_fraction(&g) < 0.0f);
    wrf(HB_CLOCK_SCALE_EA, 1.0f);
    wr32(HB_PAUSE_COUNT_EA, 1); wr32(HB_HUD_FORCE_EA, 1); hb_read(&g); assert(g.hud_hidden);
    wr32(HB_PAUSE_COUNT_EA, 0); wr32(HB_HUD_FORCE_EA, 0);
    hb_read(&g); assert(!g.hud_hidden && hb_fraction(&g) > 0.0f);

    /* Dead: no hit points left (the game leaves -1 there). */
    wrf(hero + HB_HERO_HP, -1.0f); hb_read(&g); assert(hb_fraction(&g) < 0.0f);
    wrf(hero + HB_HERO_HP, 0.0f); hb_read(&g); assert(hb_fraction(&g) < 0.0f);

    /* Numbers that are not health; health above the maximum reads as full. */
    wrf(hero + HB_HERO_HP, 100.0f);
    wrf(hero + HB_HERO_MAX_HP, 0.0f); hb_read(&g); assert(hb_fraction(&g) < 0.0f);
    wrf(hero + HB_HERO_MAX_HP, NAN); hb_read(&g); assert(hb_fraction(&g) < 0.0f);
    wrf(hero + HB_HERO_MAX_HP, 400.0f);
    wrf(hero + HB_HERO_HP, NAN); hb_read(&g); assert(hb_fraction(&g) < 0.0f);
    wrf(hero + HB_HERO_HP, 450.0f); hb_read(&g); assert(hb_fraction(&g) == 1.0f);

    /* Not the hero: another object's vtable, a null or wild pointer. */
    wrf(hero + HB_HERO_HP, 100.0f);
    wr32(hero, HB_HERO_VTABLE + 8u); hb_read(&g); assert(!g.hero && hb_fraction(&g) < 0.0f);
    wr32(hero, HB_HERO_VTABLE);
    wr32(HB_HERO_PTR_EA, 0); hb_read(&g); assert(!g.hero);
    wr32(HB_HERO_PTR_EA, 0x00000100u); hb_read(&g); assert(!g.hero);
    wr32(HB_HERO_PTR_EA, hero + 1u); hb_read(&g); assert(!g.hero);
    wr32(HB_HERO_PTR_EA, 0xFFFFFFF0u); hb_read(&g); assert(!g.hero);
    wr32(HB_HERO_PTR_EA, 0xD0000000u); hb_read(&g); assert(!g.hero);     /* past the guest heap: never read */
    wr32(HB_HERO_PTR_EA, hero); hb_read(&g); assert(g.hero && near(hb_fraction(&g), 0.25f, 1e-6f));
    munmap(vm_base, size);
}

static void test_knobs(void)
{
    unsetenv("HB_TEST_KNOB");
    assert(hb_env_int("HB_TEST_KNOB", 35, 5, 90) == 35);
    setenv("HB_TEST_KNOB", "50", 1);  assert(hb_env_int("HB_TEST_KNOB", 35, 5, 90) == 50);
    setenv("HB_TEST_KNOB", "2", 1);   assert(hb_env_int("HB_TEST_KNOB", 35, 5, 90) == 5);
    setenv("HB_TEST_KNOB", "400", 1); assert(hb_env_int("HB_TEST_KNOB", 35, 5, 90) == 90);
    setenv("HB_TEST_KNOB", "abc", 1); assert(hb_env_int("HB_TEST_KNOB", 35, 5, 90) == 35);
    setenv("HB_TEST_KNOB", "", 1);    assert(hb_env_int("HB_TEST_KNOB", 35, 5, 90) == 35);
}

int main(void)
{
    test_rate_and_strength();
    test_state_machine();
    test_game_state();
    test_knobs();
    printf("test_heartbeat: all passed\n");
    return 0;
}
