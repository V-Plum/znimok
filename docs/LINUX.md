# Linux (Wayland / PipeWire) — оцінка (ZK-109)

Стан на 01.10.2026, версія 0.0.2. Це оцінка, а не план робіт: за PLAN.md Linux робимо лише тоді, коли
з'явиться попит. Документ відповідає на три питання: що з Znimok уже переноситься, що треба написати
наново і де Linux не дасть того самого, що Windows і macOS.

## Коротко

- **Ядро переноситься без змін.** Модель і редактор (`znimok-core`), формат `.znimok`, рендер (vello_cpu +
  parley), експорт GIF, звіт для розробника, IPC/MCP, CLI, i18n, налаштування — чистий Rust без OS-коду.
  Slint і wgpu (Vulkan) на Linux працюють.
- **Платформний шар — з нуля.** Усе, що в Znimok сидить за трейтами `znimok-platform` (захоплення, вікна,
  курсор, гарячі клавіші, трей, буфер, сповіщення, дозволи, автозапуск, асоціації) і `znimok-video`
  (джерело кадрів, звук, кодувальник, декодер), — для Linux окремий крейт, як `znimok-win` і `znimok-mac`.
- **Wayland обмежує.** Захоплення йде лише через портали (`xdg-desktop-portal`): користувач підтверджує
  джерело в системному діалозі, а позиції чужих вікон і їхній список програмі недоступні. Оверлей
  «вибір вікна кліком» і «запис вікна, що рухається» у звичному вигляді неможливі; на X11 можливі, але X11
  відходить.
- **Оцінка:** знімки без вибору вікна — **5–7 тижнів** однієї людини; відео — ще **6–9 тижнів**; пакування
  (Flatpak) і CI — **2 тижні**. Разом ≈ 3–4 місяці до паритету «мінус те, що забороняє Wayland».
- **Рекомендація:** не починати без попиту. Якщо почнемо — лише Wayland через портали, лише Flatpak, спершу
  знімки (v1), потім відео.

## Що переноситься як є

| Частина | Крейти | Примітка |
|---|---|---|
| Модель, редактор, undo, команди | `znimok-core` | без OS-коду |
| Формат `.znimok`, `.zreport` | `znimok-format`, `znimok-report` | без OS-коду |
| Рендер позначок, текст | `znimok-render` (vello_cpu, parley) | шрифти вбудовані |
| Інтерфейс | `znimok-app/ui` (Slint, winit, wgpu) | Wayland і X11 підтримує сам winit; wgpu — Vulkan |
| GIF, оцінки, звіт | `znimok-export` (частина без MF), `znimok-video::gifenc` | MP4 — див. відео |
| IPC, MCP, CLI, агенти | `znimok-ipc`, `znimok-agents`, `znimok-cli` | Unix-сокет замість named pipe (`interprocess` уміє обидва) |
| i18n, налаштування, журнал | `znimok-i18n`, `znimok-settings`, `znimok-log` | шляхи — XDG (`~/.config`, `~/.cache`, `~/.local/share`) |
| OCR | помічник Tesseract (ZK-120) | той самий помічник, зібраний під Linux; моделі ті самі |
| Розширення DevTools | `extension/`, `znimok-devtools` | маніфест хоста в `~/.config/{google-chrome,chromium,microsoft-edge}/NativeMessagingHosts/` |

## Що треба написати: трейти платформи

| Трейт | Linux-механізм | Обмеження Wayland | Оцінка |
|---|---|---|---|
| `Capture` (знімок екрана/ділянки) | портал `Screenshot` (`org.freedesktop.portal.Screenshot`) → PNG; або `ScreenCast` + один кадр PipeWire | діалог порталу; у GNOME — свій інтерактивний вибір ділянки | 1–1,5 тиж. |
| `WindowList`, вибір вікна кліком | на Wayland **немає** (список і координати чужих вікон закриті); X11 — `_NET_CLIENT_LIST` | вікно обирається в діалозі порталу `ScreenCast` (тип WINDOW), не кліком у нашому оверлеї | — |
| Оверлей «заморожений екран» | знімок порталом → наше повноекранне вікно поверх | layer-shell є в KDE/wlroots, **немає в GNOME** — там звичайне повноекранне вікно | 1 тиж. |
| `Cursor` (позиція, картинка) | лише в потоці `ScreenCast` (cursor mode «metadata») | поза записом позиції курсора немає | у відео |
| `Hotkeys` (глобальні) | портал `GlobalShortcuts` (KDE 5.27+, GNOME 48+) | користувач підтверджує прив'язку; на старіших — ні | 0,5 тиж. |
| `Tray` | StatusNotifierItem (`ksni`) | GNOME показує лише з розширенням AppIndicator | 0,5 тиж. |
| `Clipboard` | `wl-clipboard`/`arboard` (wayland-data-control); X11 — selections | «копіювати як файл» — `text/uri-list` + `image/png` | 0,5 тиж. |
| `Share`, `Shell` | `xdg-open`, портал `OpenURI`, портал `FileChooser` | — | 0,5 тиж. |
| `Notifications` | портал `Notification` / `org.freedesktop.Notifications` | — | 0,3 тиж. |
| `Permissions` | у Flatpak — дозволи порталів; токен відновлення `ScreenCast` (`restore_token`), щоб не питати щоразу | KDE і GNOME зберігають по-різному | 0,5 тиж. |
| `Autostart` | портал `Background` (Flatpak) або `~/.config/autostart/*.desktop` | — | 0,2 тиж. |
| `FileAssoc` | `.desktop` + `shared-mime-info` XML для `application/x-znimok`, `.zreport` | — | 0,3 тиж. |
| Мініатюри у файловому менеджері | thumbnailer `.thumbnailer` (виклик CLI) | Nautilus/Dolphin — свої кеші | 0,3 тиж. |
| DnD назовні | winit/Slint DnD назовні на Wayland ще сирий | можливо, лише «копіювати як файл» | ризик |

## Що треба написати: відео

| Частина | Linux-механізм | Ризик | Оцінка |
|---|---|---|---|
| `FrameSource` | портал `ScreenCast` → потік PipeWire (`pipewire` крейт); кадри — dmabuf або пам'ять | імпорт dmabuf у wgpu/Vulkan ще не в стабільному API wgpu → поки копія через CPU (4K60 на межі) | 2–3 тиж. |
| `AudioSource` | PipeWire: монітор виходу (системний звук) і мікрофон | просто, як WASAPI loopback | 1 тиж. |
| `VideoSink` (H.264) | VA-API (Intel/AMD) через GStreamer `vah264enc` або `ffmpeg`; NVIDIA — NVENC; запасний — `x264` (GPL!) або `openh264` (BSD, від Cisco, гірша якість) | ліцензія: x264 GPL несумісний з нашою; патенти H.264 у дистрибутивах | 2 тиж. |
| MP4-мукс, AAC | GStreamer `mp4mux` + `fdkaacenc`/`avenc_aac`, або власний мукс (уже є перевірка `znimok-video::check::mp4`) | AAC-кодеки в дистрибутивах часто вирізані | 1 тиж. |
| `VideoDecoder`, відтворення | GStreamer `decodebin` + VA-API → кадри у wgpu (як `znimok-play`) | ті самі питання dmabuf | 1,5 тиж. |
| Курсор і кліки в записі | курсор — метадані `ScreenCast`; кліки глобально на Wayland **недоступні** | лише курсор, без кілець кліків | 0,5 тиж. |
| «Запис цього вікна» з розширення | обрати вікно в діалозі `ScreenCast` | без автоматичного пошуку за заголовком | 0,3 тиж. |

Найпростіший шлях для відео — **GStreamer** як одна залежність (захоплення PipeWire, VA-API, мукс,
декодування) замість кількох окремих бібліотек. Ціна — велика системна залежність; у Flatpak вона йде з
рантайму GNOME/Freedesktop, тож для користувача безкоштовна.

## Пакування й оновлення

- **Flatpak (Flathub)** — основний шлях: портали — його рідний спосіб доступу до екрана, рантайм
  Freedesktop дає GStreamer і VA-API, оновлення робить сам Flathub (наш оновлювач і Sparkle не потрібні).
  Підпис — ключ Flathub.
- **AppImage** — за бажанням, без пісочниці; оновлення — `AppImageUpdate` (zsync) з GitHub Releases.
- `.deb`/`.rpm` — не варто: залежності GStreamer різняться між дистрибутивами.
- CI: job `ubuntu-24.04` з `cargo build`, тести без екрана (`xvfb`/`weston --headless` для Slint),
  `flatpak-builder` у контейнері. Портали в CI не перевірити — лише ручна перевірка на GNOME і KDE.

## Чого на Linux не буде (Wayland)

1. Вибір вікна **кліком** у нашому оверлеї та підсвітка вікна під курсором — лише системний діалог.
2. Знімок без жодного діалогу з першого разу — перше використання завжди питає (далі — `restore_token`).
3. Кільця кліків у записі — глобальні кліки на Wayland недоступні.
4. Знімок «вікна без перекриття» (WGC/SCK уміють) — портал віддає вміст вікна лише в `ScreenCast`, не в
   `Screenshot`.
5. Гарячі клавіші на старих GNOME (< 48) — лише через налаштування системи (користувач прив'язує команду
   `znimok capture`).

## Порядок, якщо почнемо

1. `znimok-linux`: XDG-шляхи, Unix-сокет IPC, трей, буфер, сповіщення, автозапуск — і застосунок
   запускається (бібліотека, редактор, відкриття файлів).
2. Знімок порталом `Screenshot` + наш оверлей поверх замороженого кадру (ділянка, весь екран).
3. Гарячі клавіші порталом `GlobalShortcuts`, Flatpak-маніфест, CI.
4. Відео: `ScreenCast` + PipeWire → GStreamer (VA-API → MP4), звук, курсор; відтворення.
5. Реліз на Flathub.

Перед стартом — перевірити на живих GNOME 48 і KDE Plasma 6: портал `Screenshot` (інтерактивний і ні),
`ScreenCast` з `restore_token`, `GlobalShortcuts`, трей у GNOME без розширення.
