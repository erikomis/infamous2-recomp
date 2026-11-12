/* Force-included (-include) before a lifted spu_recomp.c for the equivalence harness:
 * the real runtime headers, then SPU_DIRECT / SPU_CHUNK_GATE_OK redefined to log each transfer
 * into a running hash and count it against a budget. Both lifts of one image (old and
 * new codegen) must log the same transfers and end in the same state. */
#include "spu_context.h"
#include <setjmp.h>
extern uint64_t g_eq_hash;
extern long g_eq_budget;
extern jmp_buf g_eq_jmp;
extern uint8_t g_eq_hint[1024];
int eq_event(spu_context* ctx, uint32_t kind, uint32_t pc);   /* 1: budget spent */
static inline int eq_direct_ok(spu_context* ctx, uint32_t pc)
{
    return (pc & SPU_LS_MASK) && !g_eq_hint[(pc >> 2) & 1023u] &&
           !(ctx->int_enable && !ctx->irq_saved && (ctx->event_status & ctx->event_mask));
}
#undef SPU_DIRECT
#define SPU_DIRECT(ctx, PC, FN) do {                                          \
        const uint32_t _dpc = (uint32_t)(PC);                                 \
        (ctx)->pc = _dpc;                                                     \
        const int _spent = eq_event((ctx), 'D', _dpc);                        \
        if (!_spent && eq_direct_ok((ctx), _dpc)) SPU_DIRECT_TAIL(ctx, FN);   \
        g_spu_trampoline_fn = FN;                                             \
        return;                                                               \
    } while (0)
#undef SPU_CHUNK_GATE_OK
static inline int eq_gate(spu_context* ctx, uint32_t pc)
{
    ctx->pc = pc;
    /* a spent budget takes the trampoline exit, so the registers are written back
     * (register-locals form) before the drain loop stops the run */
    return !eq_event(ctx, 'D', pc) && eq_direct_ok(ctx, pc);
}
#define SPU_CHUNK_GATE_OK(ctx, PC) eq_gate((ctx), (uint32_t)(PC))
