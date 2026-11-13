# Cole's health in guest memory (inFAMOUS 2, NPUA80638)

What `heartbeat.c` reads, how it was found, and how to find the same things
in another Sucker Punch title (inFAMOUS 1 runs on the same engine). Found on
2026-11-13 with `elftool.py` (this folder) on the decrypted `EBOOT.elf` and
nine headless runs of a copy of a New Marais save.

## The answer

| What | Where | Notes |
|---|---|---|
| the hero object | `u32` at **`0x00A6E9E4`** (static, .bss) | 0 while there is none (boot, between a death and the respawn); a heap object otherwise, at a different address every run and after every death (`0x496FE3E0`, `0x4A384D40`, `0x498470C0`...). Its first word is the vtable `0x0093E1E0` in every state seen (title menu, cutscene, play, dead). |
| maximum hit points | `f32` at hero **`+0x7EC`** | 400.0 in this save |
| current hit points | `f32` at hero **`+0x7F0`** | 400.0 unhurt; **-1.0** once dead, until the object is replaced |
| (energy, current / maximum) | `f32` at hero `+0x1F68` / `+0x1F70` | 180 / 180 in this save; not used by the port, found on the way |
| (recovery state) | `u32` at hero `+0x1F14`, its start time at `+0x1F18` | 0 idle, 1 just hit, 2 waiting, 3 recovering; `+0x1F1C` counts `PUSH_HP_REGEN_DISABLE`. Not read by the port; step 6 |
| HUD-hide requests | `s32` at `0x00A73130 + 0x650` (viewport 0), `+0xCE0` (viewport 1) | 0 in play; 1 while loading, 2 in the cutscene seen |
| letterbox requests | `s32` at `0x00A73130 + 0x648`, `+0xCD8` | tested by the game next to the one above; stayed 0 in the runs |
| HUD forced on | `u32` at `0x00A76314` | non-zero makes the HUD ignore the three tests above; stayed 0 |
| pause menus open | `s32` at `0x00A76318` | 1 with the pause menu up |
| world clock scale | `f32` at `0x00A2C510` | the game clock at `0xA2C500` (`game_clock.c`); 0 while paused or loading, briefly below 1 for slow motion |

Health as a fraction is `hp / max`. All values are big-endian.

The game itself does **not** run the motors for low health. With
`PS3_RUMBLE_LOG=1`, its `cellPadSetActDirect` requests are short bursts that
start at a value and fall by 20 a step (`107, 87, 67, 47, 27, 7`), tied to
jumps and landings; in 16 s of standing still at 40 of 400 hit points, the
picture black and white, there was not one non-zero request.

## How it was found

Everything below is `elftool.py <EBOOT.elf> <command>`; the repo's `.venv`
has capstone for `dis`/`opd`.

### 1. The property names are in the binary

The engine exposes object fields to its scripts by name, and the names are
plain strings:

    strings -a EBOOT.elf | grep -i -E 'hit_point|health|heal|damage'
    -> MAX_HIT_POINTS, CURRENT_HIT_POINTS, ADJUST_HIT_POINTS, WOUNDED_HIT_POINT_LIMITS,
       HERO_HIT_POINT_THRESHOLD, HIT_POINTS_REMAINING, DAMAGE_HIT_POINTS, ...

`str MAX_HIT_POINTS CURRENT_HIT_POINTS` gives where each string is and which
data words point at it: `00960E38` and `00960E6C` (a second pair, `00980590`
/ `009805C4`, belongs to another class with the fields at `+0x2CC/+0x2D0`).

### 2. The property table gives the offsets

`dump 960E04 30` shows a table of 0x34-byte records. The catch: **the name
is the last word of a record**, so a name labels the fields *before* it.

    00960E04: 'ROOT_SCALE'
    00960E0C: 00001172          id          \
    00960E10: 00000003          type 3 = f32 |  this record ends with the next
    00960E14: 0000000A          kind: plain  |  name: it is MAX_HIT_POINTS
    00960E1C: 000007EC          offset      /
    00960E38: 'MAX_HIT_POINTS'
    00960E40: 00001173
    00960E44: 00000003          f32
    00960E48: 00000012          kind: field with a setter
    00960E50: 000007F0          offset          -> CURRENT_HIT_POINTS
    00960E54: 009CBED8          setter (a function descriptor)
    00960E6C: 'CURRENT_HIT_POINTS'
    00960E74: 00001174 ... 009CE8D8   a method  -> ADJUST_HIT_POINTS

Reading the name as the record's first word gives the wrong pairing (maximum
at +0x7F0). The code settles it:

- `opd 9CBED8` -> `func_007009C4` -> `func_00363904(this, f1)`, which stores
  `f1` at `+0x7F0` (and compares it with the two floats at `+0x800`, the
  wounded limits): the setter of the *current* hit points.
- `opd 9CE8D8` -> `func_0052218C` -> `func_00363E54(this, delta)`:

      lfs   f1, 0x7f0(r27)      current
      fadds f1, f31, f1         + delta
      lfs   f13, 0x7ec(r27)     maximum
      fsubs f0, f1, f13
      fsel  f1, f0, f13, f1     min(current + delta, maximum)
      bl    0x363904            set current

  so `+0x7EC` is the maximum and `+0x7F0` the current value.

The same rule read off another table was confirmed the same way: the widget
bytes `HIDE_ON_LETTERBOX` (+0x30C) and `HIDE_DURING_PAUSE_MENU` (+0x30D), see
step 4.

### 3. A static pointer to the hero

Which object is Cole? Code that needs *the hero's* health has to fetch him
from a global. `pair 48 7f0 7ec` lists the 14 places that load both floats
(opcode 48 is `lfs`) close together; the first is the AI condition whose
parameters are `HERO_HIT_POINT_THRESHOLD` (+0x68) and
`HERO_MAGIC_POINT THRESHOLD` (+0x6C) in step 1's tables:

    000c15f0  lwz  r30, -0x7e7c(r2)     this function's constant pool
    ...
    000c1694  lwz  r9, -0x7fe4(r30)
    000c1698  lwz  r3, 0(r9)            the hero
    000c169c  cmpwi cr7, r3, 0
    000c16a0  beq  cr7, ...
    000c16a8  lfs  f0, 0x68(r31)        the threshold
    000c16ac  lfs  f13, 0x7ec(r3)
    000c16b0  fmuls f0, f0, f13         threshold * max
    000c16b4  lfs  f12, 0x7f0(r3)
    000c16b8  fcmpu cr7, f12, f0        current <= threshold * max ?
    000c16c0  lfs  f0, 0x6c(r31)        ... and the same with +0x1F68/+0x1F70 (energy)

The TOC is `0x009DB730` (the second word of any function descriptor), so
`pool 9DB730 -7e7c -7fe4` resolves the load: the pool is `0x009A97A8` and
`[pool-0x7FE4] = 0x00A6E9E4`. `refs A6E9E4` finds 58 constant pools holding
that address: a global used all over the game, as the hero would be. (In the
same pool, `-0x8000` is `0x00A2C500`, the game clock.)

### 4. When to stay silent: the game's own HUD test

Cole can be hurt when a cutscene starts, and the pause menu must stop the
beat. The game already decides when its HUD shows. The UI widgets have a
script flag `HIDE_ON_LETTERBOX`; `str HIDE_ON_LETTERBOX` -> its record gives
the byte `+0x30C`, and `disp 34 30c 30d` (opcode 34 is `lbz`) finds the one
function that tests it, `func_00465580`:

    if (widget[0x30C] && *(u32*)0xA76314 == 0)                  HIDE_ON_LETTERBOX
        if (func_00196800(-1) || func_00196898(-1) || fabs(clock.scale) < 1e-4)
            return hidden;
    if (widget[0x30D] && *(s32*)0xA76318 > 0) ...               HIDE_DURING_PAUSE_MENU

`func_00196800(-1)` is `[0xA73130+0x648] + [0xA73130+0xCD8] > 0` and
`func_00196898(-1)` the same with `+0x650` / `+0xCE0` (one counter per
viewport, 0x690 apart); `clock.scale` is `0xA2C510`. The globals come out of
the function's pool with `pool 9DB730 -79c8 -7f0c -7f00 -7fe8` and
`pool 9DB730 -7d30 -7fcc`. `heartbeat.c` repeats this test (`hb_read`).

### 5. Checking it in the running game

`PS3_HEARTBEAT_LOG=2` prints the reading whenever it changes. Runs: a copy
of the save `save-backup-20261015-0124` (New Marais pier), loaded with ✕ at
95/110/125/140 s; in play from 145-185 s (it varies with the machine's
load), then left stick right + ✕ every 2 s, which takes Cole over the pier's
rail into the river.

- **Stable while idle:** 400.0/400.0 from the title menu through the load,
  the cutscene and minutes of play.
- **Drops when hurt:** shallow water takes it down in small steps
  (`400.0 -> 398.0 -> 396.2`, `130.0 -> 129.3`), deep water kills
  (`-1.0`), 0.2-0.8 s later.
- **Death and respawn:** `-1.0` for about 2.5 s, the hero pointer goes to 0
  for half a second, then a **new object** at another address with
  400.0/400.0. So the pointer has to be re-read every time, never cached.
- **The game shows it:** a build with `-DPORT_HEARTBEAT_POKE=ON` takes
  `PS3_HEARTBEAT_POKE=<sec>:<hp>,...` and writes the current hit points at
  those times. The game's picture follows the number (`proof/run5-*.jpg`,
  each a few seconds after the write): full colour at 300 of 400, a little
  blood on the edges at 200 (50%), more at 170 (42%), colours fading at 150
  (38%), black and white with heavy blood at 130 (32%) and below, where the
  hint "Get energy and heal yourself by draining electricity [L2]" also
  comes up. So the picture is fully grey just under the heartbeat's 35%.
- **Hidden HUD:** the reading says "hud-hidden" from the load to the end of
  the cutscene (`hudhide` 1, then 2) and with the pause menu up (`pause=1`,
  `scale=0`), and not in play.
- **Recovers when he heals:** see step 6; the game's own recovery takes the
  value from 130.0 back to 400.0 in 7.8 s.

### 6. The game's recovery, and why a written value alone stays put

Hit points written from outside stayed where they were (40.0 for 26 s, the
picture grey all the while): writing a number is not a hit, and the game's
recovery is a small state machine that only a hit starts. Found from the
script methods `PUSH_HP_REGEN_DISABLE` / `POP_HP_REGEN_DISABLE` (`str`, then
`opd` on their records: they count at hero `+0x1F1C`) and `disp 32 1f1c`,
which leads to `func_00156884`:

    state at +0x1F14, set by func_00153670(this, state), which also stores
    the game clock's tick count ([0xA2C504], 1/1440 s) at +0x1F18
      1  just hit (set in func_00157A2C)  -> 2 on the next update
      2  waiting: when +0x1F1C == 0 and 10.0 s have passed -> 3
      3  recovering: 75 hit points a second below half the maximum, 30 above
         (func_001578F8, 0x1579D4: 0.5, 30.0, 75.0), until current >= maximum -> 0

The check build's poke therefore also writes 1 to `+0x1F14`, as a hit does.
Run 7 (`proof/run7.log`, frames `proof/run7-*.jpg`):

    t=148.44  poke: hit points 400.0 -> 130.0
    t=148.45  on: health 32% (130.0/400.0), 75 bpm          picture grey, 14 beats in 10.5 s
    t=158.67  health 132.0                                   the game's recovery starts, 10.2 s after the "hit"
    t=158.96  off: health back (153.5/400.0)                 38%: 3% over the threshold
    t=166.47  health 400.0                                   75/s to 200, then 30/s
    t=185.42  poke: 400.0 -> 40.0;  on: health 10%, 120 bpm
    t=188.43  off: cutscene or menu                          START: pause=1, scale=0
    t=193.66  on: health 10% (40.0/400.0), 120 bpm           START again
    t=200.74  health 42.4 ...                                recovery (10 s of game time: the pause does not count)
    t=202.21  off: health back (152.3/400.0)                 the beat slowed from 120 to 70 bpm on the way
    t=209.33  health -1.0                                    the river

Run 9 repeated run 7 with the final code: on at 148.92 (75 bpm), off
"health back" at 159.21 (152.3), on at 185.91 (120 bpm), off with the pause
menu at 188.91, on at 194.30, off "health back" at 202.65.

And run 6 (`proof/run6-death.log`): beating at 40.0, `37.6` in the shallows,
`off: dead (-1.0/400.0)` 60 ms later, a new hero object 3.2 s after that.

With `PS3_RUMBLE_LOG=1` the same logs show what reaches the motors: the
toolkit's `host pulse port 0: lo 32065, hi 11222 -> motors 32065, 11222`
for a first beat at 32%, `lo 19239, hi 0` for the second, zeros between.

## Karma: not found yet

Both ports want the light bar to follow Cole's karma. It did not turn up
next to the hit points; what was seen on the way, none of it checked in a
running game:

- `KARMA_CLASS` is a property in the same table as the hit points (record
  ending at `0x00962F88`, getter `func_001F1618`): it returns the `s32` at
  hero **`+0x1F90`**, except that the value 1 means "work it out"
  (`func_001F14B8`, which answers 2 or 3). A candidate for good / evil /
  neutral as the hero's model uses it; the values' meaning is unknown.
- `KARMA` (record ending at `0x0096EF48`) is not Cole's: its getter
  `func_001D5B74` looks a record up by id in a manager at `0x00B03210`
  (`func_00391B84`) and returns the signed byte at `+0x32` of it, which
  reads like a mission's or a choice's karma.
- Other names to start from: `KARMA_SIDE`, `SET_KARMA_SIDE`,
  `START_KARMA_TRANSITION`, `KARMA_CHANGE`, `Good_Karma`, `Evil_Karma`,
  `v_karma_good_1..3` / `v_karma_evil_1..3` (the rank visuals; the HUD of
  this save says "Guardian").

A save on the other side of the meter, or one karma choice made in a run
with a logger on hero `+0x1F90`, would settle the first lead.

## Redoing it on inFAMOUS 1

Same engine, older build: expect the same names and the same method, other
numbers.

1. `strings` the decrypted EBOOT for `HIT_POINTS`; `str MAX_HIT_POINTS
   CURRENT_HIT_POINTS`, then `dump` a little before the first reference and
   read the records with the name-is-last rule. Confirm with the setter
   (`opd <the word after the offset>`): the function that ends in
   `min(current + delta, max)` tells which offset is which.
2. `pair 48 <cur> <max>` and look for the site that loads an object from a
   global, null-checks it and multiplies the maximum by a float of its own
   object (the `HERO_HIT_POINT_THRESHOLD` condition). Take the TOC from any
   function descriptor (`opd`), then `pool <TOC> <off of the lwz rN,..(r2)>
   <off of the global's lwz>`. `refs <global>` should list dozens of pools.
3. Run with a logger on `[global] + cur/max` (copy `heartbeat.c`, change the
   `HB_*` addresses, set `HB_HERO_VTABLE` from the first log line or drop
   the check until it is known) and hurt the hero: water is lethal in both
   games and needs no enemy.
4. For the HUD test, `str HIDE_ON_LETTERBOX`, `disp 34 <its byte offset>`
   and read the function that tests it; if the name is missing, the world
   clock's scale (the game clock is in the same pool as the hero, see
   `game_clock.c`) and a pause counter are the minimum.
5. `-DPORT_HEARTBEAT_POKE=ON` + `PS3_HEARTBEAT_POKE` shows in a minute
   whether the game's picture follows the value. For the game's recovery to
   follow too, the poke has to mark a hit: find the state word as in step 6
   (`str PUSH_HP_REGEN_DISABLE`, `opd` on its record, `disp 32 <counter>`)
   and set `HB_HERO_REGEN_STATE`; without it the value simply stays.

The pattern, the toolkit call (`cellPad_host_pulse`) and the tests carry
over unchanged.
