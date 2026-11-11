#!/usr/bin/env python3
"""The layout of DoD's user messages, read from both ends in the Linux builds.

A user message is a byte stream: the server writes it field by field
(`MESSAGE_BEGIN(dest, gmsgX)`, `WRITE_BYTE(...)`, ..., `MESSAGE_END()`), and
the client's `MsgFunc_X` reads it back with `READ_BYTE()` and friends. Neither
end has a struct for it, so the layout is the order of those calls. `dod.so`
and `client.so` keep their symbols (see `docs/goldsrc_linux_debug_symbols.md`),
which names every `gmsg*` global, every engine-table slot and every
`MsgFunc_*`, so the order can be read straight out of the code:

    writers [NAME...]   per `MESSAGE_BEGIN` site in `dod.so`: the message, the
                        function writing it, and its WRITE_* calls in order
    readers [NAME...]   per `MsgFunc_*` in `client.so`: its READ_* calls in order

`{...}*` marks calls inside a loop: a backward jump inside the message spans
them. It is read from the instruction order, not a control-flow graph, and a
field behind an `if` looks the same as any other: treat the layout as where to
look, and read the function before relying on a repeated or optional field. `call NAME` is a helper called between MESSAGE_BEGIN and
MESSAGE_END, which writes more of the message itself.

Usage:
    python goldsrc-hooks/tools/user_message_layouts.py writers|readers [NAME...] [--install PATH]

NAME filters by message name (case-insensitive substring). Requires
`pyelftools` and `capstone`.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import linux_so  # noqa: E402

try:
    import capstone
except ImportError:  # pragma: no cover - developer tooling
    sys.exit("needs `pip install capstone`")

ENGINE_TABLE = "g_engfuncs"
ENGINE_STRUCT = "enginefuncs_s"
REGISTRAR = "_Z16LinkUserMessagesv"  # LinkUserMessages(): every REG_USER_MSG
READ_FUNCTIONS = {
    "READ_BYTE": "BYTE", "READ_CHAR": "CHAR", "READ_SHORT": "SHORT", "READ_LONG": "LONG",
    "READ_FLOAT": "FLOAT", "READ_STRING": "STRING", "READ_COORD": "COORD",
    "READ_ANGLE": "ANGLE", "READ_HIRESANGLE": "HIRESANGLE", "BEGIN_READ": None,
}
CLOBBERED_BY_CALL = ("eax", "ecx", "edx")
# A message whose MESSAGE_BEGIN takes no `gmsg*` global: an engine message such
# as SVC_TEMPENTITY, or an id passed in from the caller.
NOT_A_GMSG = "(engine or passed-in id)"
# How far past a reader's last READ_* a loop's backward jump may sit (the
# loop's own increment and compare).
READER_LOOP_SLACK = 0x40


def _disassembler():
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = False
    return md


def _refs_in(lib, ins):
    """Relocated targets inside one instruction: [("abs", target) | ("ext", name) | ("call", target)]."""
    out = []
    for place in range(ins.address, ins.address + ins.size - 3):
        if place in lib.abs_refs:
            out.append(("abs", lib.abs_refs[place]))
        elif place in lib.ext_refs:
            out.append(("ext", lib.ext_refs[place]))
        elif place in lib.call_refs:
            out.append(("call", lib.call_refs[place]))
    return out


def _direct_call_target(lib, ins, refs):
    if ins.mnemonic != "call":
        return None
    for kind, target in refs:
        if kind == "call":
            return target
    if ins.op_str.startswith("0x"):
        target = int(ins.op_str, 16)
        fn = lib.function_at(target)
        return target if fn and fn[0] == target else None
    return None


def function_events(lib, md, start, size, labels):
    """The calls a function makes, in instruction order, with loop and branch marks.

    `labels(lib, ins, refs, registers)` names an instruction's event (or None);
    it may keep a {register: label} map to follow `mov reg, [slot]; call reg`.
    Returns [(address, label)] and the list of (from, to) backward jumps.
    """
    events, backward, registers = [], [], {}
    for ins in md.disasm(bytes(lib.image[start:start + size]), start):
        refs = _refs_in(lib, ins)
        label = labels(lib, ins, refs, registers)
        if label:
            events.append((ins.address, label))
        if ins.mnemonic == "call":
            for reg in CLOBBERED_BY_CALL:
                registers.pop(reg, None)
        if ins.mnemonic.startswith("j") and ins.op_str.startswith("0x"):
            target = int(ins.op_str, 16)
            if start <= target < ins.address:
                backward.append((ins.address, target))
    return events, backward


def render(events, backward, region):
    """`A, {B, C}*, D` from events and the backward jumps spanning them.

    Only a jump that starts and lands inside `region` (first, last address)
    counts as a loop: GCC moves rare blocks after the function and jumps back,
    which would otherwise wrap half a message in a false loop.
    """
    first, last = region
    loops_in = [(src, dst) for src, dst in backward if first <= dst and src <= last]
    out = []
    for addr, label in events:
        loops = [(src, dst) for src, dst in loops_in if dst <= addr <= src]
        out.append((addr, label, loops[0] if loops else None))
    parts, i = [], 0
    while i < len(out):
        loop = out[i][2]
        if loop is None:
            parts.append(out[i][1])
            i += 1
            continue
        group = []
        while i < len(out) and out[i][2] == loop:
            group.append(out[i][1])
            i += 1
        parts.append("{" + ", ".join(group) + "}*")
    return ", ".join(parts)


# ── Server side: dod.so ──────────────────────────────────────────────────────

def _engine_slots(lib, path):
    layouts = linux_so.struct_layouts(path, [ENGINE_STRUCT])
    _, rows = layouts[ENGINE_STRUCT]
    base, size = lib.objects[ENGINE_TABLE]
    return base, size, {offset: member for kind, offset, _, member in rows if kind == "member"}


def _writer_labeller(lib, engine):
    base, size, slots = engine
    messages = {addr: name for name, (addr, _) in lib.objects.items() if name.startswith("gmsg")}

    def label(lib, ins, refs, registers):
        for kind, target in refs:
            if kind == "abs" and target in messages:
                return "msg:" + messages[target][4:]
            if kind == "abs" and base <= target < base + size:
                slot = slots.get(target - base, f"+{target - base:#x}")
                if ins.mnemonic in ("call", "jmp"):
                    return "eng:" + slot
                if ins.mnemonic == "mov" and "," in ins.op_str:
                    registers[ins.op_str.split(",")[0].strip()] = slot
                return None
        # `jmp reg` is a tail call: GCC ends many writers with `jmp MESSAGE_END`.
        if ins.mnemonic in ("call", "jmp") and ins.op_str in registers:
            return "eng:" + registers[ins.op_str]
        if ins.mnemonic == "mov" and "," in ins.op_str:
            registers.pop(ins.op_str.split(",")[0].strip(), None)
        target = _direct_call_target(lib, ins, refs)
        if target is not None:
            return "call:" + linux_so.demangle(lib.function_at(target)[2])
        return None

    return label


def registered_messages(lib, md, engine):
    """{gmsg global: (registered name, size)} from `LinkUserMessages`.

    Each registration is `gmsgX = REG_USER_MSG("Name", size)`: the string and
    the size go into the argument slots, the call returns the id, and the next
    store to a `gmsg*` global keeps it. The name on the wire (what a demo and
    the client see) is the string, which is not always the global's name
    (`gmsgPlayersInArea` registers "PlayersIn"). Size -1 means variable length.
    """
    base, size, slots = engine
    messages = {addr: name for name, (addr, _) in lib.objects.items() if name.startswith("gmsg")}
    start, length, _ = lib.function_by_name[REGISTRAR]
    out, text, arg_size, registers, pending = {}, None, None, {}, None
    for ins in md.disasm(bytes(lib.image[start:start + length]), start):
        refs = [t for kind, t in _refs_in(lib, ins) if kind == "abs"]
        ops = [o.strip() for o in ins.op_str.split(",")]
        for target in refs:
            if lib.cstring(target) is not None and target not in messages:
                text = lib.cstring(target)
            elif target in messages and pending:
                out[messages[target]] = pending
                pending = None
            elif base <= target < base + size and slots.get(target - base) == "pfnRegUserMsg":
                pending = (text, arg_size)
        if ins.mnemonic == "mov" and len(ops) == 2 and not refs:
            value = registers.get(ops[1], int(ops[1], 16) if ops[1].startswith("0x") or ops[1].isdigit() else None)
            if ops[0] == "dword ptr [esp + 4]":
                arg_size = None if value is None else (value - (1 << 32) if value & 0x80000000 else value)
            else:
                registers[ops[0]] = value
        elif ins.mnemonic == "xor" and len(ops) == 2 and ops[0] == ops[1]:
            registers[ops[0]] = 0
    return out


def message_writes(lib, md, engine):
    """[(message, function, layout)] for every MESSAGE_BEGIN ... MESSAGE_END in dod.so.

    `message` is the registered name with the size it was registered with,
    and the `gmsg*` global's name when it differs.
    """
    labeller = _writer_labeller(lib, engine)
    registered = registered_messages(lib, md, engine)

    def wire_name(global_name):
        name, size = registered.get("gmsg" + global_name, (global_name, None))
        suffix = "" if name == global_name else f" (gmsg{global_name})"
        return f"{name} [size {size}]{suffix}"

    out = []
    for start, size, mangled in lib.functions:
        events, backward = function_events(lib, md, start, size, labeller)
        message, body = None, None
        for addr, label in events:
            if label.startswith("msg:"):
                message = label[4:]
            elif label == "eng:pfnMessageBegin":
                body, begin = [], addr
            elif label == "eng:pfnMessageEnd" and body is not None:
                layout = render(body, backward, (begin, addr))
                out.append((wire_name(message) if message else NOT_A_GMSG, linux_so.demangle(mangled), layout))
                body = None
            elif body is not None and (label.startswith("eng:pfnWrite") or label.startswith("call:")):
                field = label[len("eng:pfnWrite"):].upper() if label.startswith("eng:") else "call " + label[5:]
                body.append((addr, field))
    return out


# ── Client side: client.so ───────────────────────────────────────────────────

def _reader_labeller(lib):
    readers = {start: READ_FUNCTIONS[linux_so.demangle(mangled)]
               for start, _, mangled in lib.functions if linux_so.demangle(mangled) in READ_FUNCTIONS}

    def label(lib, ins, refs, registers):
        target = _direct_call_target(lib, ins, refs)
        if target is None:
            return None
        if target in readers:
            return readers[target]
        return "call " + linux_so.demangle(lib.function_at(target)[2])

    return label


def message_reads(lib, md):
    """[(message, function, layout)] for every `MsgFunc_*` in client.so that reads fields."""
    labeller = _reader_labeller(lib)
    out = []
    for start, size, mangled in lib.functions:
        name = linux_so.demangle(mangled)
        if "MsgFunc_" not in name:
            continue
        events, backward = function_events(lib, md, start, size, labeller)
        fields = [(a, label) for a, label in events if label and not label.startswith("call ")]
        if not fields:
            continue
        message = name.split("MsgFunc_", 1)[1]
        region = (fields[0][0], fields[-1][0] + READER_LOOP_SLACK)
        out.append((message, name, render(events, backward, region)))
    return out


def _matches(message, needles):
    return not needles or any(n.lower() in message.lower() for n in needles)


def main(argv):
    install = linux_so.DEFAULT_INSTALL
    if "--install" in argv:
        i = argv.index("--install")
        install = Path(argv[i + 1])
        argv = argv[:i] + argv[i + 2:]
    if not argv or argv[0] not in ("writers", "readers"):
        sys.exit(__doc__)
    needles, md = argv[1:], _disassembler()
    if argv[0] == "writers":
        path = linux_so.library_path("dod", install)
        lib = linux_so.SharedObject(path)
        rows = message_writes(lib, md, _engine_slots(lib, path))
    else:
        lib = linux_so.SharedObject(linux_so.library_path("client", install))
        rows = message_reads(lib, md)
    for message, function, layout in sorted(rows):
        if _matches(message, needles):
            print(f"{message}\t{function}\t{layout}")


if __name__ == "__main__":
    main(sys.argv[1:])
