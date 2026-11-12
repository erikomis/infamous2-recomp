/* Equivalence harness for lifted SPU code (tools/spu_equiv/run_eq.sh): compiled once per image and per codegen form,
 *   cc -O1 -fno-strict-aliasing -fwrapv -I<tk>/runtime/spu -I<tk>/include \
 *      -include spu_equiv_pre.h -DLIFTED='"<img>/spu_recomp.c"' spu_equiv_main.c -o x
 *   ./x [seeds] > out.txt
 * Runs every function of the image's table on seeded random register files and local
 * stores (and random interrupt / stack-hint state, so the trampoline side of every
 * SPU_DIRECT / SPU_GATE is taken too) and prints, per function and seed, one hash of
 * every runtime call (with the register file at that moment), every cross-chunk
 * transfer, the trampoline chain the drain would run, and the final registers, LS, pc
 * and status. Two forms of the same image must print the same lines. */
#include LIFTED
#include <dlfcn.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>

uint64_t g_eq_hash;
long g_eq_budget;
jmp_buf g_eq_jmp;
uint8_t g_eq_hint[1024];
static int s_exit_kind;
static uint64_t s_rng;

/* the runtime's globals the inline helpers read */
int g_spu_ls_dbg = 0;
int g_spu_ls_watch_n = 0;
volatile int g_spu_direct = 1;
uint8_t g_spu_stack_reset_hint[1024];

static inline void mix(uint64_t v) { g_eq_hash = (g_eq_hash ^ v) * 0x100000001B3ull; g_eq_hash ^= g_eq_hash >> 29; }
static void mix_regs(const spu_context* c) {
    for (int i = 0; i < 128; i++) { mix(c->gpr[i]._u64[0]); mix(c->gpr[i]._u64[1]); }
}
static void bail(int kind) { s_exit_kind = kind; longjmp(g_eq_jmp, 1); }

int eq_event(spu_context* ctx, uint32_t kind, uint32_t pc)
{
    (void)ctx;
    mix(kind); mix(pc);
    return --g_eq_budget <= 0;
}

static const char* fname(void (*f)(spu_context*))
{
    Dl_info di;
    if (f && dladdr((void*)f, &di) && di.dli_sname) return di.dli_sname;
    return f ? "?" : "-";
}
static void mix_str(const char* s) { while (*s) mix((uint8_t)*s++); }

/* runtime entry points, as calls the lifted code makes */
void spu_ls_watch_slow(const struct spu_context* c, uint32_t lsa, int w, const uint8_t* p, uint32_t pc, uint32_t lr)
{ (void)c; (void)lsa; (void)w; (void)p; (void)pc; (void)lr; }
void spu_register_function(uint32_t addr, void (*fn)(spu_context*)) { (void)addr; (void)fn; }
u128 spu_rdch(spu_context* ctx, uint32_t ch)
{
    mix('R'); mix(ch); mix(ctx->pc); mix_regs(ctx);
    u128 r; r._u64[0] = g_eq_hash; r._u64[1] = g_eq_hash * 0x9E3779B97F4A7C15ull;
    if (--g_eq_budget <= 0) bail(2);
    return r;
}
uint32_t spu_rchcnt(spu_context* ctx, uint32_t ch) { mix('C'); mix(ch); mix_regs(ctx); return (uint32_t)(g_eq_hash & 3); }
void spu_wrch(spu_context* ctx, uint32_t ch, u128 v)
{
    mix('W'); mix(ch); mix(v._u64[0]); mix(v._u64[1]); mix(ctx->pc); mix_regs(ctx);
    if (--g_eq_budget <= 0) bail(2);
}
void spu_stop(spu_context* ctx) { mix('S'); mix(ctx->stop_code); mix(ctx->pc); mix_regs(ctx); }
void spu_halt(spu_context* ctx) { mix('H'); mix_regs(ctx); bail(3); }
void spu_img_restore(spu_context* ctx, int32_t saved) { mix('I'); mix((uint32_t)saved); ctx->image_id = saved; }
void spu_indirect_branch(spu_context* ctx)
{
    mix('B'); mix(ctx->pc); mix(ctx->host_depth); mix_regs(ctx);
    ctx->tramp_fn = 0;
}
void spu_indirect_branch_mt(spu_context* ctx) { spu_indirect_branch(ctx); }
/* the drain after a lifted call: run the pending trampoline chain, by name */
static void run_tramp(spu_context* ctx)
{
    while (ctx->tramp_fn) {
        void (*f)(spu_context*) = ctx->tramp_fn;
        ctx->tramp_fn = 0;
        mix('T'); mix_str(fname(f)); mix(ctx->pc);
        if (--g_eq_budget <= 0) { mix_regs(ctx); bail(2); }
        f(ctx);
    }
}
void spu_drain_call(spu_context* ctx, uint32_t ret)
{
    mix('X'); mix(ret); mix(ctx->pc); mix_str(fname(ctx->tramp_fn));
    run_tramp(ctx);
}
void spu_trace_init(const char* p) { (void)p; }
void spu_trace_pc(spu_context* c, uint32_t pc) { (void)c; (void)pc; }
void spu_trace_rt(spu_context* c, uint32_t rt) { (void)c; (void)rt; }

static uint32_t rnd32(void) { s_rng ^= s_rng << 13; s_rng ^= s_rng >> 7; s_rng ^= s_rng << 17; return (uint32_t)(s_rng >> 11); }
static uint32_t rnd_word(void)
{
    switch (rnd32() % 8) {
    case 0: return rnd32();                                     /* anything */
    case 1: return rnd32() & 0x3FFF0u;                          /* an LS address */
    case 2: return rnd32() % 64u;                               /* a small count / index */
    case 3: return 0;
    case 4: return ((rnd32() % 40u) + 107u) << 23 | (rnd32() & 0x807FFFFFu); /* an ordinary float */
    case 5: return rnd32() & 0x807FFFFFu;                       /* zero / denormal */
    case 6: return 0xFFFFFFFFu;
    default: return (rnd32() & 1) ? 0x3F800000u : 0x00010203u + 0x04040404u * (rnd32() % 4u); /* 1.0f, a shufb control */
    }
}

static void on_alarm(int sig) { (void)sig; s_exit_kind = 4; longjmp(g_eq_jmp, 1); }
static void on_segv(int sig) { (void)sig; s_exit_kind = 5; longjmp(g_eq_jmp, 1); }

int main(int argc, char** argv)
{
    const int seeds = argc > 1 ? atoi(argv[1]) : 3;
    const char* only = argc > 2 ? argv[2] : 0;
    static spu_context c;
    static uint8_t ls[SPU_LS_SIZE] __attribute__((aligned(128)));
    static uint8_t ls0[SPU_LS_SIZE];
    static uint8_t altstack[1 << 16];
    stack_t ss = { .ss_sp = altstack, .ss_size = sizeof altstack, .ss_flags = 0 };
    sigaltstack(&ss, 0);
    struct sigaction sa; memset(&sa, 0, sizeof sa); sa.sa_handler = on_segv; sa.sa_flags = SA_ONSTACK | SA_NODEFER;
    sigaction(SIGSEGV, &sa, 0); sigaction(SIGBUS, &sa, 0);
    struct sigaction sb; memset(&sb, 0, sizeof sb); sb.sa_handler = on_alarm; sb.sa_flags = SA_NODEFER;
    sigaction(SIGALRM, &sb, 0);
    s_rng = 0x1234567887654321ull;
    for (int i = 0; i < SPU_LS_SIZE; i += 4) { uint32_t w = rnd_word(); memcpy(ls0 + i, &w, 4); }
    int n = 0;
    for (const spu_func_entry* e = spu_function_table; e->func; ++e) {
        if (only && strcmp(only, e->name)) continue;
        for (int sd = 0; sd < seeds; sd++) {
            s_rng = 0xA5A5A5A55A5A5A5Aull ^ ((uint64_t)e->addr << 20) ^ (uint64_t)(sd * 7919 + 1);
            memset(&c, 0, sizeof c);
            memcpy(ls, ls0, SPU_LS_SIZE);
            for (int k = 0; k < 64; k++) { uint32_t a = rnd32() & 0x3FFFCu, w = rnd_word(); memcpy(ls + a, &w, 4); }
            c.ls = ls;
            for (int r = 0; r < 128; r++) for (int k = 0; k < 4; k++) c.gpr[r]._u32[k] = rnd_word();
            c.gpr[1]._u32[0] = 0x3F000u + (rnd32() & 0xF00u);   /* a plausible stack pointer */
            c.pc = e->addr; c.status = SPU_STATUS_RUNNING; c.host_depth = 1 + (rnd32() & 1);
            c.image_id = 7; c.code_base = 0;
            /* the trampoline side: some stack hints, an enabled interrupt now and then */
            memset(g_eq_hint, 0, sizeof g_eq_hint);
            if (sd > 0) for (int k = 0; k < 60; k++) g_eq_hint[rnd32() & 1023u] = 1;
            c.int_enable = sd > 1 && (rnd32() & 1); c.event_mask = rnd32() & 0x30u; c.event_status = rnd32() & 0x30u;
            g_eq_hash = 0xCBF29CE484222325ull; g_eq_budget = 3000; s_exit_kind = 0;
            struct itimerval it = { { 0, 0 }, { 0, 200000 } };
            if (!setjmp(g_eq_jmp)) {
                setitimer(ITIMER_REAL, &it, 0);
                e->func(&c);
                run_tramp(&c);
                s_exit_kind = 1;
            }
            struct itimerval z = { { 0, 0 }, { 0, 0 } }; setitimer(ITIMER_REAL, &z, 0);
            if (s_exit_kind >= 4) { printf("%s %d SKIP%d\n", e->name, sd, s_exit_kind); fflush(stdout); continue; }
            mix(s_exit_kind); mix_regs(&c); mix(c.pc); mix(c.status); mix(c.host_depth); mix(c.int_enable);
            mix((uint32_t)c.image_id); mix(c.stop_code);
            for (int i = 0; i < SPU_LS_SIZE; i += 8) { uint64_t w; memcpy(&w, ls + i, 8); if (w) { mix(i); mix(w); } }
            printf("%s %d %d %016llx\n", e->name, sd, s_exit_kind, (unsigned long long)g_eq_hash);
        }
        n++;
    }
    fprintf(stderr, "%d functions\n", n);
    return 0;
}
