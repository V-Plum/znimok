# -*- coding: utf-8 -*-
"""ZK-17 (P4): усі виміри відтворення однією командою (Windows) і звірка з критеріями.

  python crates/znimok-p4/tools/p4_check.py [шлях до znimok-p4.exe] [--dir ТЕКА] [--real ФАЙЛ.mp4]

Тестові кліпи генеруються самим прототипом (Media Foundation, H.264 High, як записує Little
Helpers) у ТЕКУ (за замовч. %TEMP%\\znimok-p4) і перевикористовуються. Кожен кадр несе свій номер
штрих-кодом, тож правильність кадру після перемотки перевіряється без еталонного файлу.

1. selftest — NV12→RGB у WGSL проти CPU-формули (±1), штрих-код читається.
2. info — D3D11-пристрій декодера на тому самому адаптері (LUID), що й wgpu.
3. 4K60 без копіювання: ≥ 60 к/с; з --verify кожен кадр правильний; рівно 60 к/с — CPU ≤ 10 %.
4. Запасні шляхи: staging через CPU (cpu), наївний MF Lock (mflock), програмний декодер (sw).
5. Перемотка 4K: GOP = 60 (як у LH) і GOP = 15 з --lowlat; ≤ 100 мс і кадр точний.
6. (опційно --real) справжній запис: швидкість і перемотка.
Вихід — рядки OK/FAIL/INFO і підсумок; код виходу 0 лише якщо все OK.
"""
import json, os, subprocess, sys, tempfile

sys.stdout.reconfigure(encoding="utf-8")

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
args = sys.argv[1:]


def opt(name):
    if name in args:
        i = args.index(name)
        v = args[i + 1]
        del args[i:i + 2]
        return v
    return None


DIR = opt("--dir") or os.path.join(tempfile.gettempdir(), "znimok-p4")
REAL = opt("--real")
EXE = args[0] if args else os.path.join(ROOT, "target", "release", "znimok-p4.exe")
os.makedirs(DIR, exist_ok=True)
fails = 0


def check(what, ok, extra=""):
    global fails
    print(f"  {'OK  ' if ok else 'FAIL'} {what}{(' — ' + extra) if extra else ''}", flush=True)
    fails += 0 if ok else 1


def info(what, extra=""):
    print(f"  INFO {what}{(' — ' + extra) if extra else ''}", flush=True)


def run(*a):
    r = subprocess.run([EXE, *a], capture_output=True, text=True, encoding="utf-8")
    if r.returncode != 0:
        raise RuntimeError(f"{' '.join(a)}: {r.stderr.strip()}")
    return json.loads(r.stdout)


def clip(name, *gen_args):
    path = os.path.join(DIR, name)
    if not os.path.exists(path):
        g = run("gen", "-o", path, *gen_args)
        info(f"згенеровано {name}", f"{g['size'][0]}×{g['size'][1]}, {g['frames']} кадрів, GOP {g['gop']}, {g['mbit_s']} Мбіт/с, апаратний кодер: {g['encoder_hardware']}")
    return path


print("1. selftest")
st = run("selftest")
check(f"шейдер NV12→RGB = CPU ±1 на {st['adapter']}", st["ok"], f"max_diff {st['max_diff']}, штрих-код {st['barcode']}")

print("2. адаптер")
i = run("info")
same = i["d3d11_on_same_adapter"]
check("wgpu на DX12 з NV12-текстурами", i["backend"] == "Dx12" and i["nv12_textures"], f"{i['wgpu_adapter']}")
check("D3D11 декодера на тому самому адаптері (за LUID)", "error" not in same, json.dumps(same, ensure_ascii=False))

k4 = clip("4k60-g60.mp4", "--seconds", "20")
k4g15 = clip("4k60-g15.mp4", "--seconds", "10", "--gop", "15")

print("3. 4K60 без копіювання")
v = run("bench", k4, "--mode", "zero", "--verify")
check("кожен кадр правильний (штрих-код), кольори ±3", v["verify"]["wrong_index"] == 0 and v["verify"]["damaged_barcode"] == 0 and v["verify"]["patch_max_diff"] <= 3, json.dumps(v["verify"]))
check("кадри декодуються в пам'ять GPU (DXVA)", v["decoded_frames_in"] == "gpu (DXVA)", v["decoded_frames_in"])
f = run("bench", k4, "--mode", "zero")
check("≥ 60 к/с", f["fps_achieved"] >= 60, f"{f['fps_achieved']} к/с")
p = run("bench", k4, "--mode", "zero", "--paced", "--frames", "600")
check("рівно 60 к/с: CPU ≤ 10 % машини", p["cpu_percent_of_machine"] <= 10 and p["fps_achieved"] >= 59.5, f"{p['cpu_percent_of_machine']} % машини ({p['cpu_percent_of_one_core']} % одного ядра), запізнилось {p['frames_late_over_one_period']} з 600")

print("4. запасні шляхи (4K)")
c = run("bench", k4, "--mode", "cpu", "--verify", "--frames", "300")
check("cpu (staging): кожен кадр правильний", c["verify"]["wrong_index"] == 0 and c["verify"]["damaged_barcode"] == 0, json.dumps(c["verify"]))
for m in ("cpu", "mflock", "sw"):
    r = run("bench", k4, "--mode", m)
    info(f"{m}: {r['fps_achieved']} к/с", f"CPU {r['cpu_percent_of_machine']} % машини")

print("5. перемотка 4K (GPU-шлях)")
s60 = run("seek", k4, "--gop", "60")
check("GOP 60: кадр завжди точний", s60["wrong_frame"] == 0, f"{s60['seeks']} перемоток")
info(f"GOP 60 (як LH): медіана {s60['ms_p50']} мс, p95 {s60['ms_p95']}, макс {s60['ms_max']} мс", "≤ 100 мс " + ("так" if s60["ms_max"] <= 100 else "НІ — див. GOP 15"))
s15 = run("seek", k4g15, "--gop", "15", "--lowlat")
check("GOP 15 + низька затримка: кадр точний і ≤ 100 мс", s15["wrong_frame"] == 0 and s15["ms_max"] <= 100, f"медіана {s15['ms_p50']}, p95 {s15['ms_p95']}, макс {s15['ms_max']} мс")
sc = run("seek", k4g15, "--gop", "15", "--lowlat", "--mode", "cpu")
check("запасний шлях: перемотка теж точна", sc["wrong_frame"] == 0, f"медіана {sc['ms_p50']}, макс {sc['ms_max']} мс")

if REAL:
    print("6. справжній запис")
    b = run("bench", REAL, "--mode", "zero")
    info(f"{b['size'][0]}×{b['size'][1]} @ {b['fps']}: {b['fps_achieved']} к/с", f"CPU {b['cpu_percent_of_machine']} %")
    s = run("seek", REAL)
    check("перемотка ≤ 100 мс", s["ms_max"] <= 100, f"медіана {s['ms_p50']}, макс {s['ms_max']} мс (номерів кадрів у записі немає — точність тут не перевіряється)")

print(f"\nПідсумок: {'усе OK' if fails == 0 else f'{fails} FAIL'}")
sys.exit(1 if fails else 0)
