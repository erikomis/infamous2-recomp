/*
 * macOS process and thread priority for the game (main.cpp calls these).
 *
 * App Nap / timer throttling: macOS slows down an app it thinks the user is
 * not waiting on -- a window behind others, a fullscreen Space the user swiped
 * away from, a run with the lid half shut -- by coalescing its timers and
 * lowering its threads. The guest's 3-10 us polls and the 16 ms frame clock
 * are exactly what that breaks. An NSProcessInfo activity with
 * NSActivityUserInitiated | NSActivityLatencyCritical, held for the whole run,
 * tells the system the work is user-initiated and latency-critical (it also
 * keeps idle system sleep away, as tools/boot.sh's caffeinate -i did).
 *
 * QoS: a new pthread starts at QOS_CLASS_DEFAULT, not at its creator's class,
 * so each frame-critical thread asks for USER_INTERACTIVE itself (P-cores, and
 * ahead of background work). The toolkit does the same for the PPU and SPU
 * threads it creates (runtime/platform/host_qos.h); this covers the threads
 * the port creates: the main thread, the guest's main thread and the frame
 * clock (RSX FIFO, present). Opt-in, PS3_QOS=1: the headless A/B showed no
 * frame-time gain and two hangs on the frame fence in three runs with it
 * (host_qos.h). The App Nap activity is always on.
 */
#import <Foundation/Foundation.h>
#include <pthread.h>
#include <pthread/qos.h>
#include <stdio.h>
#include <stdlib.h>

static id s_activity;   /* held for the life of the process */

void infamous2_host_begin_activity(void)
{
    if (s_activity) return;
    @autoreleasepool {
        s_activity = [[NSProcessInfo processInfo]
            beginActivityWithOptions:(NSActivityUserInitiated | NSActivityLatencyCritical)
                              reason:@"inFAMOUS 2 recomp: game running"];
    }
}

int infamous2_host_qos_enabled(void)
{
    static int on = -1;
    if (on < 0) {
        const char* e = getenv("PS3_QOS");
        on = e && e[0] == '1';
    }
    return on;
}

void infamous2_host_qos_self(void)
{
    if (infamous2_host_qos_enabled())
        pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0);
}
