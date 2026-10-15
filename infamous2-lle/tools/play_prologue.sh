#!/bin/sh
# Play the prologue by script, headless: tools/play_prologue.sh [name] [seconds]
#
# Boots, starts a new game, lets the docks cutscene run and answers the
# tutorial with timed stick holds and button presses: the right stick in each
# direction (look up / left / right / down, three cycles so the prompt is met
# whenever it appears), the left stick in each direction, then the left stick
# held forward for "Head towards the disturbance", and L1 held with R1 taps
# for "Hold L1 to aim" at the pier gate. Frames land in out/<name>/ every
# 10 s, the log in out/<name>.log.
#
# Times are wall-clock seconds and were measured on an M-series Mac with
# nothing else running; the cutscene ends around 460-520 s and the gate is
# reached around 750 s. The same prompt has landed 150 s apart in two clean
# runs, so every hold is generous. Another load on the machine shifts
# everything later still.
here=$(cd "$(dirname "$0")/.." && pwd)
name=${1:-prologue}; secs=${2:-1200}
# lx,ly,rx,ry,from,until (0-255, 128 = centred); up to 16 holds.
PAD_STICK="128,128,128,0,470,480;128,128,0,128,480,490;128,128,255,128,490,500;128,128,128,255,500,510"
PAD_STICK="$PAD_STICK;128,128,128,0,510,520;128,128,0,128,520,530;128,128,255,128,530,540;128,128,128,255,540,550"
PAD_STICK="$PAD_STICK;128,128,128,0,550,560;128,128,0,128,560,570;128,128,255,128,570,580;128,128,128,255,580,590"
PAD_STICK="$PAD_STICK;128,0,128,128,590,605;0,128,128,128,605,620;255,128,128,128,620,635;128,0,128,128,640,$secs"

# sec:mask[:hold] -- cross 0x4000, R1 0x0800, L1 0x0400, R2 0x0200; up to 32.
PAD_SCRIPT="95:0x4000,110:0x4000"                                    # menu: new game, confirm
PAD_SCRIPT="$PAD_SCRIPT,700:0x4000,720:0x4000"                      # a jump or two on the way
PAD_SCRIPT="$PAD_SCRIPT,760:0x0400:400"                             # aim, held through the gate
PAD_SCRIPT="$PAD_SCRIPT,765:0x0800,772:0x0800,779:0x0800,786:0x0800,793:0x0800,800:0x0800"   # bolts
PAD_SCRIPT="$PAD_SCRIPT,810:0x0800,820:0x0800,830:0x0800,840:0x0800,850:0x0800,860:0x0800"
PAD_SCRIPT="$PAD_SCRIPT,880:0x0800,900:0x0800,920:0x0800,940:0x0800,960:0x0800,980:0x0800"
PAD_SCRIPT="$PAD_SCRIPT,1000:0x0800,1030:0x0800,1060:0x0800,1090:0x0800,1120:0x0800,1150:0x0800"

PAD_STICK="$PAD_STICK" PAD_SCRIPT="$PAD_SCRIPT" exec "$here/tools/capture.sh" "$name" "$secs"
