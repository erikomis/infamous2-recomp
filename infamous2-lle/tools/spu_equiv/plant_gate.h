/* planted bug: the chunk gate forgets the stack-reset hint */
#undef SPU_CHUNK_GATE_OK
static inline int eq_gate_bad(spu_context* ctx, uint32_t pc)
{
    ctx->pc = pc;
    return !eq_event(ctx, 'D', pc) && (pc & SPU_LS_MASK) && !(ctx->int_enable && !ctx->irq_saved && (ctx->event_status & ctx->event_mask));
}
#define SPU_CHUNK_GATE_OK(ctx, PC) eq_gate_bad((ctx), (uint32_t)(PC))
