# Clearing GoldSrc decals at runtime

How `dodstudio_clear_decals` works, and why the pipeline's existing decal
hygiene had to be so much more elaborate. Answers the R&D question in #290.

Everything here is from offline analysis of the pre-Anniversary `hw.dll`
(`pefile` + `capstone`, the house method in
`docs/goldsrc_client_dll_internals.md` §10), checked by
`goldsrc-hooks/tools/verify_decal_offsets.py`.

---

## 1. Why `r_decals` was never going to do this

`r_decals` bounds a rotating index and **evicts nothing**:

```c
limit = min(r_decals, MAX_RENDER_DECALS)
if (gDecalCount >= limit) gDecalCount = 0;
pdecal = &gDecalPool[gDecalCount++];
R_DecalUnlink(pdecal);          // the ONLY path that clears an old decal
```

A decal is removed only when the ring index lands on its slot. Lowering the
cvar mid-demo strands every decal sitting above the new limit — the index can
no longer reach them, so they stay on the wall permanently while new decals
churn through the small surviving window.

That is the whole reason for `native/src/patch/decal_strip.rs`: pin the ring
small at demo load, then inject a full revolution's worth of synthetic decals
into the gap before each clip so the index walks past every real one. It works.
It also costs a demo rewrite per capture, forces `r_decals` to be set exactly
once from `init_commands`, and carries the standing rule that it must never be
injected mid-demo because an extra `ConsoleCommand` frame shifts every later
frame ordinal by +1 (`docs/goldsrc_dod_quirks.md`).

---

## 2. What the engine actually has

#290 guessed the order right — "a map change clears decals, so the code to do
it exists; finding and calling it is far safer than zeroing the pool by hand".
Three functions matter, and they are adjacent:

| | what it does |
|---|---|
| `hw+0x49e80` `R_DecalUnlink(decal_t *)` | detaches one decal from its surface's list. A NULL `psurface` returns early, so a free slot is safe to pass. |
| `hw+0x49da0` `R_DecalInit()` | `memset` the whole pool, `gDecalCount = 0`, and reset the 256-entry decal texture table to -1. |
| `hw+0x4a000` remove-all-by-flag | for each slot, if `flags & mask`: unlink, then zero 28 bytes. |

Measured from the instruction stream, not assumed:

```
gDecalPool    0x2325cb8 .. 0x2341cb8      (0x1c000 bytes)
sizeof(decal_t)  0x1c                      4096 slots = MAX_RENDER_DECALS
gDecalCount   0x2342308
decal_t::psurface  +0x04
decal_t::flags     +0x16                   bit 0 is "permanent"; the allocator
                                           skips `flags & 0x81` when rotating
```

---

## 3. The trap: `R_DecalInit` alone is a crash

It is the obvious thing to call — one function, no arguments, clears
everything. It wipes the pool **without unlinking**, so every
`msurface_t::pdecals` still pointing into it becomes a chain of zeroed
structures the renderer will keep walking.

That is fine on level load, which is the only place the engine calls it, because
the surfaces are being rebuilt in the same breath. Mid-demo it is a use of
freed structure contents.

What makes the engine's own *remove* functions safe is that they unlink first.
So `dodstudio_clear_decals` is that loop with the flag test dropped:

```rust
for slot in pool {
    R_DecalUnlink(slot);              // no-op for a slot with no surface
    write_bytes(slot, 0, DECAL_SIZE);
}
gDecalCount = 0;
```

Nothing is invented — it is byte-for-byte what `hw+0x4a000` does, minus
`if (flags & mask)`. Dropping that test is deliberate: a decal whose flags are
zero would survive `R_DecalRemoveAll(0xffff)`, and "clear all" that leaves some
behind is worse than not having it.

The flag test being dropped also means **permanent decals go too**
(`FDECAL_PERMANENT`, bit 0 — what the allocator refuses to recycle). For
capture that is the point.

---

## 4. Nothing is hardcoded, and the call is identified by name

Two signatures, each matching once in the pre-Anniversary `hw.dll`. Every value
the DLL uses is an immediate or a relative call *inside* a matched span:

- the remove loop gives the pool base, the pool end and `R_DecalUnlink`;
- `R_DecalInit` gives the pool base again and `gDecalCount`.

They cross-check: both must name the same pool base, and the span the loop
walks must equal the `memset` length `R_DecalInit` passes. Three independent
statements of the same number.

The verifier goes further and identifies `R_DecalUnlink` **without any
address**: it is the only function in the image that references the string
`"Bad decal list"`, which is what it prints when a decal is not in its
surface's list. The function found that way is the one at the end of the
relative call the DLL resolves.

---

## 5. The 25th Anniversary build (#370)

`R_DecalInit`'s signature matches the Anniversary engine too (`+0x2484d0`).
The remove loop's does **not**: that build inlines `R_DecalUnlink` into its
remove loops, so the loop leads nowhere. It still keeps a standalone
`R_DecalUnlink` (`+0x2492a0`, called from `R_DecalCreate`), and that is what
`decals.rs` finds there, by the function's own first instructions
(`ANNI_UNLINK`). Those work out the decal's pool index for the decal cache
(`sub ecx, <pool>`), so they also give the pool base, which must equal
`R_DecalInit`'s. The pool's end is the base plus the length `R_DecalInit`
clears.

The two builds agree on everything the clear relies on: a 28-byte `decal_t`,
`psurface` at +4, `msurface_t::pdecals` at +0x58, 4096 decals, and an unlink
that invalidates the decal's cache entry as well as detaching it.
`verify_decal_offsets.py --anniversary` checks all of that.

---

## 6. What this does not replace yet

`dodstudio_clear_decals` is a console command that clears on demand. The capture
pipeline still does its own thing, because wiring the two together is a
separate decision:

- the pipeline would schedule the command ahead of each clip instead of
  injecting a flush burst, which removes the demo rewrite, the `r_decals`
  load-time constraint and the ordinal-shift rule at once;
- but the flush burst is live-proven and this is not, so nothing was switched
  over on the strength of a static analysis.

The obvious first live test is: load a demo, shoot a wall, `dodstudio_clear_decals`,
and see whether the holes go and the game keeps running.
