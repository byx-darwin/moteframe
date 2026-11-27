"""Run the actual ARM driver against a register-level FR8029D/P25Q80 test double."""

from pathlib import Path
import argparse, json, os, re, shutil, struct, subprocess, sys, tempfile
from unicorn import (
    Uc,
    UC_ARCH_ARM,
    UC_MODE_THUMB,
    UC_MODE_MCLASS,
    UC_HOOK_MEM_READ,
    UC_HOOK_MEM_WRITE,
    UC_HOOK_CODE,
)
from unicorn.arm_const import (
    UC_ARM_REG_PC,
    UC_ARM_REG_LR,
    UC_ARM_REG_R0,
    UC_ARM_REG_SP,
    UC_ARM_REG_PRIMASK,
)
from elftools.elf.elffile import ELFFile

if not __debug__:
    raise RuntimeError("Offline validation needs Python assertions enabled")

fixture = Path(__file__).resolve().parent
board = fixture.parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("output", type=Path, help="directory for ELFs and size report")
args = parser.parse_args()
d = args.output.resolve()
d.mkdir(parents=True, exist_ok=True)
sys.path.insert(0, str(board / "tools"))
from gatt_notify_memory import inspect_image

Q, C, X, R = 0xE00A4000, 0xE00A0000, 0x10000000, 0x20000000
S = 0x72000
size = 8192
initial = (
    bytes([0xA9]) * S + bytes([255]) * size + bytes([0x73]) * (0x100000 - S - size)
)


def run(
    path, kind, stage, media, cut=0, layout=S, wrong_id=False, stuck=False, sim=False, polling=0x10024005
):
    u = Uc(UC_ARCH_ARM, UC_MODE_THUMB | UC_MODE_MCLASS)
    for base, n in [(X, 0x100000), (R, 0x20000), (C, 0x8000), (0x60000000, 0x5000)]:
        u.mem_map(base, n)
    u.mem_write(X, media)
    with path.open("rb") as handle:
        elf = ELFFile(handle)
        for seg in elf.iter_segments():
            if seg["p_type"] == "PT_LOAD" and seg["p_filesz"]:
                u.mem_write(seg["p_vaddr"], seg.data())
        symbols = elf.get_section_by_name(".symtab")
        entry = symbols.get_symbol_by_name("trial")[0]["st_value"]
        stall = (
            None
            if sim
            else symbols.get_symbol_by_name("model_stall")[0]["st_value"] & ~1
        )
        assert bool(symbols.get_symbol_by_name("vTaskSwitchContext")) == (
            "rtos" in path.name
        )
        if not sim:
            ramcmd = symbols.get_symbol_by_name("fr29_slot_command")[0]
            assert R <= ramcmd["st_value"] < R + 0x18000
    if sim:
        u.mem_write(0x60000000, media[S : S + size])

    def set32(a, v):
        u.mem_write(a, struct.pack("<I", v))

    def word(a):
        return struct.unpack("<I", u.mem_read(a, 4))[0]

    for off, v in [(0, 0x80890087), (8, 2), (0x14, 0x101002), (0x38, polling)]:
        set32(Q + off, v)
    set32(C + 4, 0x12)
    set32(C, 0x3D)
    set32(0x60004000, layout)
    ctrl = {
        "sr": 0x7C,
        "wip": 0,
        "vol": False,
        "busy": 0,
        "cmd": 0,
        "cache": 2,
        "erases": [],
        "pages": [],
        "dac": 0,
        "stigs": [],
        "stalled": False,
    }

    def cmd(m, k, a, n, v, _):
        if not v & 1:
            return
        assert (
            R <= u.reg_read(UC_ARM_REG_PC) < R + 0x18000
            and u.reg_read(UC_ARM_REG_PRIMASK) == 1
        )
        op = v >> 24
        ctrl["cmd"] = v & ~1
        ctrl["busy"] = 1
        ctrl["stigs"].append(op)
        sr = ctrl["sr"]
        if op == 5:
            set32(Q + 0xA0, sr | int(ctrl["wip"] > 0))
            ctrl["wip"] = max(0, ctrl["wip"] - 1)
            return
        assert not ctrl["wip"]
        if op == 0x9F:
            set32(Q + 0xA0, 0 if wrong_id else 0x146085)
        elif op == 0x35:
            set32(Q + 0xA0, 2)
        elif op == 0x50:
            ctrl["vol"] = True
        elif op == 1:
            assert ctrl["vol"] and word(Q + 0xA8) in (0, 0x7C)
            ctrl.update(sr=word(Q + 0xA8), vol=False, wip=2)
        elif op == 6:
            assert sr == 0
            ctrl["sr"] = 2
        elif op == 4:
            ctrl["sr"] &= ~2
        elif op == 0x20:
            addr = word(Q + 0x94)
            assert (
                sr == 2
                and addr in (S, S + 4096)
                and ctrl["cache"] == 0
                and v == 0x200A0001
            )
            ctrl["erases"].append(addr)
            u.mem_write(X + addr, bytes([255]) * 4096)
            ctrl.update(sr=0, wip=3)
        else:
            raise AssertionError(hex(op))

    def busy(m, k, a, n, v, _):
        set32(a, ctrl["cmd"] | (2 if ctrl["busy"] else 0))
        ctrl["busy"] = max(0, ctrl["busy"] - 1)

    def cache(m, k, a, n, v, _):
        ctrl["cache"] = {0x38: 0, 0x3C: 0, 0x3E: 0, 0x3D: 2}[v]
        set32(C + 4, 16 | ctrl["cache"])

    def inv(m, k, a, n, v, _):
        if word(a) == 0x3E:
            set32(a, 0x3C)

    def page(m, k, a, n, v, _):
        off = a - X
        assert (
            n == 1
            and S <= off < S + size
            and ctrl["sr"] == 0
            and not ctrl["wip"]
            and ctrl["cache"] == 0
        )
        assert (
            R <= u.reg_read(UC_ARM_REG_PC) < R + 0x18000
            and u.reg_read(UC_ARM_REG_PRIMASK) == 1
        )
        assert u.mem_read(a, 1)[0] & v == v
        if ctrl["dac"] % 256 == 0:
            assert off % 256 == 0
            ctrl["pages"].append(off)
        assert off == ctrl["pages"][-1] + ctrl["dac"] % 256
        ctrl["dac"] += 1
        if not ctrl["dac"] % 256:
            ctrl["wip"] = 2
        if cut and ctrl["dac"] == cut:
            u.emu_stop()

    def trap(m, a, n, _):
        ctrl["stalled"] = True
        u.emu_stop()

    u.hook_add(UC_HOOK_MEM_WRITE, cmd, begin=Q + 0x90, end=Q + 0x90)
    u.hook_add(UC_HOOK_MEM_READ, busy, begin=Q + 0x90, end=Q + 0x90)
    u.hook_add(UC_HOOK_MEM_WRITE, cache, begin=C, end=C)
    u.hook_add(UC_HOOK_MEM_READ, inv, begin=C, end=C)
    u.hook_add(UC_HOOK_MEM_WRITE, page, begin=X, end=X + 0xFFFFF)
    if stall is not None:
        u.hook_add(UC_HOOK_CODE, trap, begin=stall, end=stall)
    sim_writes = []

    def record_sim(m, k, a, n, v, _):
        sim_writes.append(a)
        if cut and len(sim_writes) == 4096 + cut:
            u.emu_stop()

    if sim:
        u.hook_add(UC_HOOK_MEM_WRITE, record_sim, begin=0x60000000, end=0x60001FFF)
    sp = R + 0x18000
    paint = bytes([0xA5]) * 8192
    u.mem_write(sp - 8192, paint)
    arg, expected = stage, 0
    if kind == "service":
        arg, expected, fill = {
            0: (25601, 2, 1),
            1: (2, 1, 2),
            5: (99 * 256 + 2, 1, 2),
            2: (0, 1, 0),
            3: (25602, 2, 2),
            4: (0, 2, 0),
        }[stage]
        u.mem_write(0x60003000, bytes([fill]) * 300)
    if wrong_id or layout != S or polling == 0x4000:
        expected = 9
    if stuck:
        set32(Q, 0x890087)
    stop = X + 0xFF000
    u.reg_write(UC_ARM_REG_SP, sp)
    u.reg_write(UC_ARM_REG_LR, stop | 1)
    u.reg_write(UC_ARM_REG_R0, arg)
    u.emu_start(entry | 1, stop, count=10000000)
    if stuck:
        assert ctrl["stalled"] and u.reg_read(UC_ARM_REG_PRIMASK) == 1
    elif not cut:
        assert (u.reg_read(UC_ARM_REG_PC), u.reg_read(UC_ARM_REG_R0)) == (
            stop,
            expected,
        ), (kind, stage, hex(u.reg_read(UC_ARM_REG_PC)), u.reg_read(UC_ARM_REG_R0))
        assert (
            ctrl["sr"] == 124
            and ctrl["cache"] == 2
            and not ctrl["wip"]
            and u.reg_read(UC_ARM_REG_PRIMASK) == 0
        )
        if sim:
            assert len(sim_writes) == (4864 if stage in (0, 3) else 0)
        elif expected == 9 or stage not in (0, 3):
            assert (
                not ctrl["pages"]
                and not ctrl["erases"]
                and not set(ctrl["stigs"]) & {6, 0x50, 1, 0x20}
            )
        else:
            target = S + (4096 if stage == 3 else 0)
            assert ctrl["erases"] == [target] and ctrl["pages"] == [
                target,
                target + 256,
                target + 3840,
            ]
        if kind == "service" and stage in (2, 4) and expected != 9:
            assert bytes(u.mem_read(0x60003000, 300)) == bytes([stage // 2]) * 300
    elif sim:
        assert len(sim_writes) == 4096 + cut
    else:
        assert ctrl["dac"] == cut
    # The boot bytes are overlaid by the ELF; compare sentinels outside the fixture separately.
    assert bytes(u.mem_read(X + 0x40000, S - 0x40000)) == media[0x40000:S]
    assert bytes(u.mem_read(X + S + size, 0x100000 - S - size)) == media[S + size :]
    out = (
        media[:S]
        + bytes(u.mem_read(0x60000000 if sim else X + S, size))
        + media[S + size :]
    )
    peak = 8192 - next(
        (
            i
            for i, (a, b) in enumerate(zip(paint, u.mem_read(sp - 8192, 8192)))
            if a != b
        ),
        8192,
    )
    return out, peak


def build_images():
    # A private crate copy keeps all synthetic addresses and entry points out
    # of deployable firmware. Do not change the board linker script or opt level.
    with tempfile.TemporaryDirectory() as temp:
        devices = Path(temp) / "devices"
        crate = devices / "firmware" / board.name
        shutil.copytree(board, crate, ignore=shutil.ignore_patterns("target", "tools"))
        shutil.copytree(
            board.parents[1] / "runtimes" / "freertos",
            devices / "runtimes" / "freertos",
        )
        manifest = crate / "Cargo.toml"
        manifest.write_text(
            re.sub(
                r'path = "(\.\./[^"]+)"',
                lambda m: 'path = "' + str((board / m[1]).resolve()) + '"',
                manifest.read_text(),
            )
        )
        shutil.copy2(fixture / "entry.rs", crate / "src/model_entry.rs")
        shutil.copy2(fixture / "model_nor.rs", crate / "src/model_nor.rs")
        with (crate / "src/ble_adv_probe.rs").open("a") as out:
            out.write("\nmod model_entry;\n")
        env = os.environ.copy()
        env.setdefault(
            "FREERTOS_KERNEL_DIR",
            str((board / "../../target/fr8029d-freertos/kernel").resolve()),
        )
        for rt in ("bare", "rtos"):
            features = "qspi-config,ble-gatt-xip-compact,ble-gatt-event-driven"
            if rt == "rtos":
                features += ",ble-gatt-freertos"
            for kind in ("service", "full"):
                cmd = [
                    "cargo",
                    "rustc",
                    "--offline",
                    "--locked",
                    "--release",
                    "--target",
                    "thumbv8m.main-none-eabi",
                    "--target-dir",
                    str(d / "build"),
                    "--manifest-path",
                    str(manifest),
                    "--bin",
                    "fr8029d-ble-adv",
                    "--features",
                    features,
                    "--",
                    "--check-cfg",
                    "cfg(model_full)",
                    "-C",
                    "link-arg=--undefined=trial",
                ]
                if kind == "full":
                    cmd.extend(["--cfg", "model_full"])
                cmd.extend(["--check-cfg", "cfg(model_sim)"])
                for hw in ("qspi", "sim"):
                    subprocess.run(
                        cmd + (["--cfg", "model_sim"] if hw == "sim" else []),
                        check=True,
                        env=env,
                    )
                    shutil.copy2(
                        d / "build/thumbv8m.main-none-eabi/release/fr8029d-ble-adv",
                        d / f"{hw}-{kind}-{rt}.elf",
                    )


build_images()
results = {}
for hw in ("qspi", "sim"):
    for rt in ("bare", "rtos"):
        for kind in ("service", "full"):
            path = d / f"{hw}-{kind}-{rt}.elf"
            stats = inspect_image(path.read_bytes(), xip=True).report()
            peaks = []

            def check(stage, media, **kw):
                image, n = run(path, kind, stage, media, sim=(hw == "sim"), **kw)
                peaks.append(n)
                return image

            if hw == "qspi":
                for invalid in (0x1000, 0x5F000, S + 1, 0xDF000, 0xFFFFF000):
                    assert check(0, initial, layout=invalid) == initial
                assert check(0, initial, polling=0x4000) == initial
                assert check(0, initial, polling=0) != initial
                assert check(0, initial, wrong_id=True) == initial
                assert check(0, initial, stuck=True) == initial
            a = check(0, initial)
            assert check(1, a) == a and check(2, a) == a
            if kind == "service":
                assert check(5, a) == a
            partial = a
            for cut in (1, 256, 512):
                partial = check(3, partial, cut=cut)
                assert (
                    partial[S : S + 4096] == a[S : S + 4096]
                    and check(2, partial) == partial
                )
            b = check(3, partial)
            assert b[S : S + 4096] == a[S : S + 4096] and check(4, b) == b
            stats["storage_stack_written_peak"] = max(peaks)
            results[hw + "-" + rt + "-" + kind] = stats
            print(
                hw,
                rt,
                kind,
                "PASS",
                stats["image_payload_bytes"],
                stats["static_bytes"],
                stats["stack_reserved_total_bytes"],
                max(peaks),
                flush=True,
            )
(d / "sizes.json").write_text(json.dumps(results, indent=2))
