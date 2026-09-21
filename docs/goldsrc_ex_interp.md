# Raising GoldSrc's interpolation ceiling

How `dodtools_ex_interp_max` works, and why the engine's own 200 ms path does
not. Answers #271.

Offline analysis of the pre-Anniversary `hw.dll` (`pefile` + `capstone`),
checked by `goldsrc-hooks/tools/verify_ex_interp_offsets.py`.

---

## 1. `ex_interp` is engine-managed

A clamp at `hw+0x18ee0` runs every frame, forces `ex_interp` into a range, and
**writes the clamped result back through `Cvar_Set`** — printing `ex_interp
forced up to %i msec` or `forced down to %i msec`. Setting the cvar by hand and
expecting it to stay does not work, and that is the engine's behaviour rather
than anything DoD does.

```asm
hw+0x18ef3  mov edi, 0x32          ; floor, 50 ms
hw+0x18ef8  mov ebx, 0x64          ; ceiling, 100 ms
hw+0x18f5f  mov eax, [0x2d5df84]   ; a flag
hw+0x18f64  test eax, eax
hw+0x18f68  mov ebx, 0xc8          ; ceiling, 200 ms, when it is set
hw+0x18f82  fld  [1000.0]
hw+0x18f88  fdiv [cl_updaterate]   ; the real floor, at least 1
            ...clamp into [edi, ebx], print, Cvar_Set it back
```

A longer window means smoother entity motion between snapshots, which is where
most spectated-demo ugliness comes from. 100 ms is a *network* compromise; a
demo being rendered offline has no latency budget to protect.

---

## 2. The 200 ms path is dead

#271 asked what the flag at `0x2d5df84` is, and reasoned that if it means "we
are playing a demo" then demos already get 200 ms and the work is worth half.

They do not. Within `hw.dll` the flag has exactly **four** write sites — found
by searching every x86 addressing form for a write to an absolute address, not
just the obvious `mov dword [x], imm32`:

| site | what | value |
|---|---|---|
| `hw+0x1087a` | `mov [flag], ebx` | in the demo reader, `ebx` zeroed |
| `hw+0x10a58` | `mov dword [flag], 0` | in the demo reader |
| `hw+0x18537` | `mov [flag], ebx` | in a block zeroing a dozen fields |
| `hw+0x1d791` | `mov dword [flag], 1` | **unreachable** |

The only site that writes 1 has:

- no direct call to it,
- no near jump to it,
- **no absolute reference anywhere in the image**, so no jump table and no
  function pointer,
- and the five bytes before it are `jmp hw+0x2a960`, an unconditional branch,
  so nothing falls through.

So the engine never takes its own 200 ms branch. The ceiling is a hard 100, and
raising it is the only way up — which makes #271 worth what it looked like
rather than half.

`verify_ex_interp_offsets.py` reproduces every one of those checks rather than
restating them.

---

## 3. The flag is not the lever

Setting it would be one dword and looks tempting. It is **read from 25 sites**,
including inside `CL_ParseServerMessage`, the `svc_*` handlers and
`CL_CheckCRCs`. Flipping a feature switch the retail build never sets, to find
out what else it turns on, is not a thing to do inside someone's capture run.

Rewriting the immediate is the narrow change: it affects the clamp and nothing
else, and it reverts by writing 100 back.

---

## 4. What still bounds it

`cl_updaterate` sets the floor as `1000 / cl_updaterate`, a few instructions
below the ceiling. A demo recorded at a low update rate cannot be interpolated
below what it captured, so raising the ceiling does **not** make a 20-tick
recording smooth. What it does is stop the engine shortening a window that was
already long enough.

Say so when reporting it, or the first response will be "it does nothing".

---

## 5. Engine-wide, deliberately

The `hw.dll` survey's standing preference is to do a thing in `client.dll`
where an equivalent exists, because an engine change affects the menu and every
mod. There is no client-side equivalent — the clamp is the engine's — and this
install exists only to render demos.

Movies install only, as everything injected is. The stock Half-Life install is
VAC-secured and nothing goes near it.

---

## 6. The setting

```
dodtools_ex_interp_max 250     raise the clamp's ceiling to 250 ms
dodtools_ex_interp_max 100     put the engine's own ceiling back
```

A cvar rather than a command, because it holds a value, and it defaults to 100
— the engine's own ceiling — so registering it changes nothing until asked.
`commands::poll` applies it every frame, which is what makes a change take
effect without a restart.

Refused: anything at or below the engine's 50 ms floor (the clamp would pin
`ex_interp` to a single value and say so once per frame), and anything above
1000 ms, which stops being smoothing and starts being a rewrite of when things
happened.

The module refuses to overwrite a ceiling that is neither the engine's 100 nor
a value it would have written itself, so a second patcher is reported rather
than clobbered.

---

## 7. Live test (2026-09-21)

The mechanism works: the user watched an HLTV demo while changing the value
and saw a real, visible effect on entity motion smoothness, confirming the
clamp is actually being raised and applied every frame as intended.

No single value has been settled on as "the right one" yet. Too high and
flickshots look wrong — the user's own description, not yet pinned down to a
specific artifact (plausibly the interpolation window stretching far enough
that a fast snap-to-target reads as sliding into place rather than a sudden
flick, since that is exactly the kind of motion a wider window smooths across
more aggressively). This is a tuning tradeoff, not a correctness bug: the
cvar defaults to the engine's own stock ceiling (100 ms) and changes nothing
until a session sets it higher, so shipping the capability is safe regardless
of what the ideal value turns out to be. Worth revisiting once a specific
value or range is confirmed to look right across more than one demo.
