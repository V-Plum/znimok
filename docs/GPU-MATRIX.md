# Матриця GPU: шляхи interop, запасні шляхи, покриття (ZK-116)

Стан на v0.0.16. Документ відповідає на три питання: **де Znimok ділить ресурси GPU між API**, **що
стається, коли на конкретному залізі це не працює**, і **що з цього перевіряє CI, а що — лише живе
залізо**. Наприкінці — ручна матриця прогонів і знайдені прогалини (окремі тікети).

Позначка *(висновок)* — виведено з коду, не перевірено запуском.

## 1. Шляхи

| # | Шлях | Де | Як | Запасний шлях | Покриття |
|---|---|---|---|---|---|
| A | Пристрій UI (Slint femtovg-wgpu): полотно й шар відео | `znimok-app/src/main.rs` (`manual_wgpu`, закріплення бекенду), `app.rs` (`gpu_lost`) | DX12 / Metal (Vulkan вимкнено — краш драйвера Intel UHD 630, ZK-14); адаптер за `WGPU_ADAPTER_NAME` або за енергопрофілем; NV12, якщо є | Не вдалось → `WGPUConfiguration::Automatic` з лімітами WebGL2 → плеєр не створить конвертер і покаже постер. Немає адаптера → застосунок не стартує (програмного рендерера немає) | Лише вручну (самотест). `ui_tests.rs` — тестовий бекенд без GPU |
| B | Тон-мапінг HDR→SDR знімків (`znimok-gpu`) | `znimok-gpu/src/tone.rs`, `znimok-app/src/capture.rs` | Власний пристрій, кадр із CPU → compute → назад | Будь-яка помилка або немає пристрою → еталон на CPU (`Frame::to_srgb8`) | Юніт-тести GPU vs CPU (±1) — у CI на WARP / Metal раннера *(висновок)* |
| C | Знімки Windows (WGC / DXGI) | `znimok-win/src/win/{mod,wgc,dxgi}.rs` | D3D11 → staging → CPU, далі B | Екран: WGC → DXGI. Вікно: лише WGC. Пристрій D3D11 лише HARDWARE | `znimok-win/tests/live.rs`, `threads.rs` — у CI |
| D | Запис Windows: міст D3D11 ↔ wgpu/D3D12 (спільні текстури й fence) | `znimok-video-win/src/interop.rs`, `source.rs` | Адаптер монітора за LUID; спільні NT-хендли текстур; спільні fence в обидва боки | Екран: WGC ↔ DDA за налаштуванням; втрачений DDA перевідкривається. **Збій моста = помилка запису**, CPU-шляху немає | `tests/pipeline.rs`, `live.rs`, `still_window.rs` — у CI на WARP *(висновок)* |
| E | Апаратний кодер H.264 Windows (MF + NV12-алокатор + D3D11 Video Processor) | `znimok-video-win/src/sink.rs`, `nv12.rs`, `shader.rs` | Sink Writer з D3D-менеджером; BGRA→NV12 у BT.709 | Не відкрився апаратний → програмний (NV12 з пам'яті, або RGB32 через конвертер MF). **NV12 не того формату посеред запису — помилка без переходу** | `pipeline.rs` — у CI, але апаратний MFT на WARP не відкривається → CI ганяє програмні шляхи *(висновок)* |
| F | Плеєр Windows: MF DXVA → спільна NV12 → пристрій UI | `znimok-play/src/win.rs`, `convert.rs`, `lib.rs` | Шляхи `gpu` / `upload` / `software` | При відкритті: немає моста → програмний декодер. Кадр не в GPU → `upload` назавжди. **Збій `shared_nv12` / `GetResource` / тайм-аут fence 2 с → відтворення мовчки зупиняється** (`lib.rs` ~598, ~667: лише `eprintln!`) | `tests/play.rs` — у CI на WARP іде `software`, zero-copy **не** перевіряється |
| G | Плеєр macOS: VideoToolbox → IOSurface → CVMetalTextureCache → wgpu | `znimok-play/src/mac.rs` | NV12 video-range, текстури Metal без копій | Немає кешу → `upload` від початку; **будь-який збій кадру → `upload` назавжди** (правильна поведінка) | `znimok-export/tests/export_mac.rs` — у CI *(висновок)*; `tests/clip.rs` потребує кліпу |
| H | Запис macOS: ScreenCaptureKit → AVAssetWriter | `znimok-video-mac/src/{recording,source,writer}.rs` | Без wgpu; масштаб і обрізання — SCK на GPU | Апарат/програма вибирає VideoToolbox; лише SDR | `tests/writer.rs` — у CI; SCStream — лише вручну (дозвіл) |
| I | Знімки macOS | `znimok-mac/src/{mac,picker}.rs` | CGImage → CPU | — | Лише вручну |
| J | Експорт і мініатюри | `znimok-play/src/lib.rs` (`headless_gpu`), `znimok-export`, `znimok-video-win/src/export.rs` | Декодування як F/G, назад у CPU, кодування | Кодер: апаратний → програмний (Windows) | `znimok-export/tests/*` — у CI |
| K | HDR/FP16 під час запису | `znimok-video-win/src/source.rs`, `shader.rs` | FP16 для вікон і HDR-дисплеїв; DDA scRGB і PQ | — | scRGB синтетично в `pipeline.rs`; **PQ / 10 біт — ніде** |

## 2. Що насправді перевіряє CI

`ci.yml`: windows-2025 і macos-26 — `fmt`, `clippy`, `cargo test --workspace`, без змінних
середовища для GPU. Раннер Windows має лише WARP (Microsoft Basic Render Driver) *(висновок з
коментарів у `play.rs`, `interop.rs`)*.

- **Перевіряється в CI:** тон-мапінг; знімки WGC/DXGI; міст запису (спільні текстури й fence на
  WARP); програмний кодер MF; програмний декодер плеєра; експорт; на Mac — кодер VideoToolbox і,
  ймовірно, zero-copy плеєра.
- **Лише на живому залізі:** апаратне декодування MF; zero-copy плеєра Windows (`shared_nv12`);
  апаратний кодер з NV12-алокатором і Video Processor; DDA 10 біт / PQ; пристрій UI; на Mac —
  запис SCStream і пристрій UI.
- Усі GPU-тести **проходять зі словом «skipped»**, якщо пристрою чи кодера немає. Раннер, що
  втратив GPU, лишиться зеленим.
- **Самотест у CI не запускається**, хоча його шапка (`selftest.rs`) каже, що запускається.

## 3. Що відомо про залізо

| Залізо | Що перевірено | Джерело |
|---|---|---|
| Intel UHD 630 (PLUM-MEDIA, i5-10500) | Основна ціль: P4 (плеєр), запис, самотест; Vulkan-драйвер падає в `request_device` → DX12 закріплено | `crates/znimok-p4/README.md`, `docs/PROTOTYPES.md` |
| NVIDIA RTX 4080 | Лише затримка P1 | `docs/PROTOTYPES.md` |
| Apple M1 Pro | P4 на macOS | `crates/znimok-p4/README.md` |
| **AMD** | **нічого** | — |
| **Гібридні ноутбуки (Optimus)** | **нічого**: адаптер UI береться за енергопрофілем, а не за GPU дисплея *(висновок)* | `crates/znimok-p4/README.md` |
| HEVC / 10 біт, декодери NVIDIA/AMD | не перевірено | там само |

Обіцянки з `docs/PLAN.md` (§ ризики): «весь hal-interop в одному модулі `znimok-gpu::interop`, CI-тест
імпорту» і «матриця GPU для тестів» — **не зроблено**: interop лежить у `znimok-play/src/{win,mac}.rs`
і `znimok-video-win/src/interop.rs`, а `znimok-gpu` робить лише тон-мапінг.

## 4. Діагностика

Користувач і підтримка **не бачать, яка відеокарта й який шлях працюють**: «Про Znimok» показує
коміт, ОС і архітектуру; звіт про збій і рядок старту логу — так само. Дані є, але викидаються:
`ToneMapper.adapter`, `interop::Gpu.name`, `Started{gpu, encoder, hardware}` запису (застосунок бере
лише розмір), `Info.path` плеєра (читає лише самотест), `Outcome.hardware` експорту.

Усі повідомлення про запасні шляхи — `eprintln!`. У релізі Windows консолі немає, а лог-файл
отримує лише `tracing`, тож **у релізних збірках переходи на запасні шляхи невидимі**.

## 5. Ручна матриця

Прогін на кожній машині — релізна збірка, самотест з відео:

```
ZNIMOK_SELFTEST=<тека> ZNIMOK_SELFTEST_VIDEO=1 ZNIMOK_LIBRARY=<тимч. тека> znimok-app <знімок.png>
```

і вручну: запис екрана 30 с (SDR, а де є — HDR), відтворення з перемоткою вперед/назад, експорт MP4
і GIF. Записати: адаптер, шлях плеєра (`gpu`/`upload`/`software`), кодер (назва MFT, апаратний чи
ні), час експорту 30 с 1080p.

| Машина | GPU | Стан |
|---|---|---|
| PLUM-MEDIA | Intel UHD 630 | основна; прогони щорелізно |
| PLUM-PC (власник) | уточнити (P1 міряли на RTX 4080 — можливо, тут) | треба прогін — після тікета діагностики (щоб бачити шлях і кодер) |
| MacBook (Apple Silicon) | M1 Pro | прогони під час робіт над Mac |
| AMD (будь-яка) | — | немає заліза; кандидат — хмарна машина або знайомий |
| Гібридний ноутбук | Intel + NVIDIA | немає заліза |

## 6. Прогалини → тікети

1. **ZK-283** — плеєр Windows: збій zero-copy посеред відтворення → перехід на `upload`, як на Mac; про
   остаточну зупинку — `Event::Failed`, а не мовчання.
2. **ZK-284** — діагностика GPU: адаптер, бекенд, шлях плеєра, кодер запису й експорту — у лог (`tracing`) і в
   «Про Znimok» (рядок + «Копіювати» для звіту про баг); усі `eprintln!` запасних шляхів →
   `tracing::warn!`.
3. **ZK-285** — CI: GPU-тести, що пропустились, — видно в підсумку (або окремий крок, що валиться, якщо на
   раннері пропало все); виправити шапку самотесту або запускати його в CI.
4. **ZK-286** — ручна матриця на PLUM-PC за §5, після п. 2.
