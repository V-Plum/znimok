# -*- coding: utf-8 -*-
"""ZK-15 (P2): усі перевірки прототипу захоплення однією командою (Windows).

  python crates/znimok-p2/tools/p2_check.py [шлях до znimok-p2.exe]

1. selftest — WGSL проти CPU-еталона формул Little Helpers (scRGB 80/240, PQ, 10-біт SDR, BGRA8): ±1.
2. Тестове вікно (testwin.py): WGC-знімок вікна → розмір = межі DWM, кольори точні, Borderless = Allowed.
3. Рамка на екрані: border-probe off → 0 жовтих пікселів; on (контроль) → є.
4. Увесь дисплей через WGC і DXGI: розмір = монітор, кадр не чорний.
Вихід — рядки OK/FAIL і підсумок; код виходу 0 лише якщо все OK.
"""
import json, os, subprocess, sys, tempfile, time

sys.stdout.reconfigure(encoding="utf-8")

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
EXE = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "target", "debug", "znimok-p2.exe")
TMP = tempfile.gettempdir()
fails = 0


def check(what, ok, extra=""):
    global fails
    print(f"  {'OK  ' if ok else 'FAIL'} {what}{(' — ' + extra) if extra else ''}", flush=True)
    fails += 0 if ok else 1


def run(*args):
    r = subprocess.run([EXE, *args], capture_output=True, text=True, encoding="utf-8")
    if r.returncode != 0:
        raise RuntimeError(f"{' '.join(args)}: {r.stderr.strip()}")
    return json.loads(r.stdout)


def pixels(path):
    from PIL import Image
    return Image.open(path).convert("RGB")


print("1. selftest (WGSL проти CPU)")
st = run("selftest")
check(f"усі випадки ±1 на {st['adapter']}", st["ok"], ", ".join(f"{c['transfer']}/{c['white']}: {c['max_diff']}" for c in st["cases"]))

print("2. знімок вікна (WGC)")
tw = subprocess.Popen([sys.executable, os.path.join(HERE, "testwin.py"), "40", "300", "200", "800", "500"], stdout=subprocess.PIPE, text=True)
tw.stdout.readline()
time.sleep(1.5)
try:
    png = os.path.join(TMP, "zk15_check_win.png")
    s = run("shot", "window", "ZK15 test window", "-o", png)
    check("Borderless = Allowed", s["capture"]["borderless_access"] == "Allowed", s["capture"]["borderless_access"])
    check("розмір кадру = межі DWM (без невидимої тіні)", s["size_matches_dwm_bounds"], f"{s['size']} / dwm {s['dwm_size']} / GetWindowRect {s['target']['window_rect']}")
    check("курсор вимкнено, формат FP16", s["capture"]["cursor_disabled"] and s["format"] == "R16G16B16A16_FLOAT")
    im = pixels(png)
    w, h = im.size
    top = next((y for y in range(h) if im.getpixel((10, y)) == (0, 255, 0)), None)
    check("зелений квадрат у куті клієнтської області", top is not None)
    if top is not None:
        check("кольори точні після FP16 → тон → sRGB",
              im.getpixel((10, top + 10)) == (0, 255, 0) and im.getpixel((100, top + 20)) == (255, 0, 0) and im.getpixel((w // 2, h // 2)) == (0, 0, 255),
              f"{im.getpixel((10, top + 10))} {im.getpixel((100, top + 20))} {im.getpixel((w // 2, h // 2))}")

    print("3. жовта рамка на екрані")
    off = run("border-probe", "ZK15 test window", "off")
    on = run("border-probe", "ZK15 test window", "on")
    check("без рамки: жовтих пікселів 0", off["yellow_pixels"] == 0, f"{off['yellow_pixels']} з {off['ring_pixels']}")
    check("контроль з рамкою: детектор її бачить", on["yellow_pixels"] > 100, f"{on['yellow_pixels']} з {on['ring_pixels']}")
finally:
    tw.kill()

print("4. увесь дисплей")
for api in ("wgc", "dxgi"):
    png = os.path.join(TMP, f"zk15_check_disp_{api}.png")
    try:
        s = run("shot", "display", "0", "--api", api, "-o", png)
    except RuntimeError as e:
        check(f"{api}: захоплення", False, str(e))
        continue
    im = pixels(png)
    small = im.resize((64, 36)).tobytes()
    lit = sum(1 for i in range(0, len(small), 3) if max(small[i:i + 3]) > 16)
    check(f"{api}: розмір = монітор, кадр не чорний", s["size_matches_monitor"] and lit > 100, f"{s['size']} {s['format']} {s['transfer']} white {s['white_nits']}")

print()
print("УСЕ ГАРАЗД" if fails == 0 else f"ПРОВАЛІВ: {fails}")
sys.exit(1 if fails else 0)
