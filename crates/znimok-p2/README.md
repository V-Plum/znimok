# znimok-p2 — прототип P2: захоплення на Windows (ZK-15)

Одноразовий вимірювальний код Фази 1. Що вціліє — переїде в `znimok-win` / `znimok-render` у Фазі 3.

```
znimok-p2 displays                                   монітори (DPI, колірний простір, біле SDR) — JSON
znimok-p2 windows                                    вікна верхнього рівня з межами DWM — JSON
znimok-p2 shot display <n> [--api wgc|dxgi] [--cpu] [-o out.png]
znimok-p2 shot window <hwnd|частина назви> [--cpu] [-o out.png]
znimok-p2 border-probe <hwnd|назва> on|off           чи видно жовту рамку WGC на екрані
znimok-p2 selftest                                   WGSL проти CPU-еталона формул LH
znimok-p2 compare a.png b.png [--tol N]              порівняти два знімки однієї сцени
```

Усі перевірки однією командою: `python crates/znimok-p2/tools/p2_check.py` (потрібні Pillow і зібраний
`target/debug/znimok-p2.exe`).

## Що зроблено

- **WGC** (`Windows.Graphics.Capture`) для дисплея й вікна: `RequestAccessAsync(Borderless)` →
  `IsBorderRequired = false`, `IsCursorCaptureEnabled = false`, пул кадрів завжди
  `R16G16B16A16Float` (scRGB: на HDR значення > 1.0, на SDR — той самий вміст, 1.0 = біле), копіюється
  лише `ContentSize`.
- **DXGI Desktop Duplication** як альтернатива для дисплея (`DuplicateOutput1` з FP16 / 10-біт / BGRA8),
  з пастками LH: перебір усіх адаптерів, пропуск першого чорного кадру, `RowPitch ≠ ширина × bpp`.
- **Тон-мапінг у WGSL** (`src/tone.wgsl`) за формулами LH: scRGB ÷ (біле/80), PQ → ніти ÷ біле з матрицею
  BT.2020→709, зріз вище білого, крива sRGB. CPU-еталон — `src/tone.rs`.
- **Біле SDR** — `DisplayConfigGetDeviceInfo` тип **11**, `SDRWhiteLevel / 1000 × 80` ніт (запас: HDR 200,
  SDR 80). Колірний простір і HDR — з `IDXGIOutput6::GetDesc1` і advanced color info.
- **Вікна** — межі `DWMWA_EXTENDED_FRAME_BOUNDS` (без невидимої тіні Windows 11), приховані (`cloaked`)
  і згорнуті пропускаються; процес Per-Monitor-v2 — усі координати фізичні.
- wgpu 30 на **DX12** (на Windows) / Metal: Vulkan-драйвер Intel UHD 630 падає в `request_device`.

## Результати на PLUM-MEDIA (Windows 11 25H2, збірка 26200, процес без прав адміністратора, RDP)

| Перевірка | Результат |
|---|---|
| WGSL проти CPU (scRGB 80 і 240 ніт, PQ 203, 10-біт SDR, BGRA8) | різниця ≤ 1 (округлення) |
| Знімок вікна | розмір = межі DWM (786×493; `GetWindowRect` 800×500 — з тінню), кольори точні |
| Borderless | `Allowed` без запиту; на екрані жовтих пікселів **0** (контроль із рамкою — 5 100) |
| Дисплей через WGC / DXGI | 3840×2089 обидва; перший кадр WGC ~60–80 мс, DXGI ~30 мс |
| Тон-мапінг 4K на GPU | ~14 мс (Intel UHD 630) |

## Що може перевірити лише власник

1. **HDR проти Little Helpers** (критерій «збігається з LH у допуску»). На HDR-моніторі, увімкнений HDR,
   на екрані щось статичне (наприклад, картинка на весь екран):
   ```
   znimok-p2 displays                                   → номер HDR-монітора, sdr_white_nits
   znimok-p2 shot display <n> -o zk.png                 → знімок Znimok
   (Little Helpers: Alt+Shift+3 → «Експорт…» → lh.png)
   znimok-p2 compare zk.png lh.png --tol 2
   ```
   Очікування: `max_diff` ≤ 2–3, `share_over_tol` близько нуля (LH рахує через LUT, тут — шейдер).
   Те саме з `--api dxgi`.
2. **Змішаний DPI** (два монітори з різним масштабом, напр. 100 % і 150 %): вікно на кожному моніторі →
   `znimok-p2 shot window "<назва>"` → `size_matches_dwm_bounds: true` на обох; `windows` показує `dpi`
   кожного вікна.
