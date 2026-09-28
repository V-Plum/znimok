### Znimok — interface strings, Ukrainian (uk).
###
### Reference file for translators, human or AI. English (en.ftl) and Ukrainian (uk.ftl) are the
### two complete references; a new language is one more file xx.ftl with the same ids — see i18n/README.md.
### Every message has an instruction comment (@where, @kind, @max, @note); it is identical in all files.
###
### Rules
###   - Audience: people who take screenshots at work. Tone: plain, friendly, short; never blame the user.
###   - Sentence case everywhere (only the first word capitalised), in every language.
###   - Buttons and menu items: an action. English — imperative ("Save"); Ukrainian — infinitive ("Зберегти").
###   - An ellipsis "…" (U+2026, one character) means the item opens a dialog before acting. Keep it.
###   - Quotes: English “…”, Ukrainian «…». Apostrophe in Ukrainian: ' (U+0027) as in the reference.
###   - Sizes are written "1920 × 1080": U+00D7 with spaces, in every language.
###   - Key names stay as on the keyboard: Ctrl, Alt, Shift, Esc, Enter, Tab, Delete, F2; macOS glyphs ⌘ ⌥ ⇧ ⌃. Single letters in tool names, e.g. "(R)", are physical keys: keep them.
###   - Never translate: Znimok, Claude, Claude Code, Claude Desktop, Anthropic, MCP, Slack, Jira, Telegram, Redmine, GitHub, Chrome, Edge, DevTools, PNG, JPEG, WebP, GIF, MP4, HEX, HDR, SDR, OCR, Slint, font names.
###   - Variables look like { $name }. Keep every variable of the English message, spelled exactly the same; you may move it inside the sentence.
###   - Plurals: use the CLDR categories of your language (English: one, other; Ukrainian: one, few, many, other). The default variant is marked with * and must be [other].
###   - @max is the most characters the text may take after variables are filled with typical values; longer text is cut in the UI. Prefer a shorter wording over an abbreviation.
###   - @kind says what the text is: button, menu, tab, option, label, heading, title, hint, body, tooltip, a11y (screen reader only), placeholder, status, toast, error, badge, value.
###   - @where says where the text is shown. Read it before translating: the same English word can need different translations in different places.
###
### Glossary (English — Ukrainian — meaning)
###   screenshot — знімок — a captured still image; also the document kind
###   annotation — позначка — anything drawn on a screenshot: rectangle, arrow, text, counter…
###   library — бібліотека — the folder with all screenshots and videos, and the home screen
###   editor — редактор — the window where a screenshot is annotated
###   over the screen — поверх екрану — editing right on the frozen screen (overlay mode)
###   overlay — накладка — the frozen-screen layer where the user chooses what to capture
###   card after a capture — плашка — the small card in the corner after a capture (6 s)
###   crop / to crop — кадр / кадрувати — a document property, not a pixel cut: annotations outside stay
###   hide (tool) — приховати — blur, pixelate or cover a part of the image
###   highlighter — маркер — semi-transparent band over text
###   counter — лічильник — numbered badge; groups number themselves
###   stamp — штамп — ready-made sign: check, cross, star…
###   copy — копіювати — the main action: image to the clipboard
###   share — поділитися — the OS share sheet
###   export — експорт — ONLY saving a file through the export dialog
###   hand to agent — передати агенту — give the document to an AI agent
###   agent — агент — an external AI program connected over MCP
###   assistant — помічник — the built-in natural-language command bar (Ctrl+K)
###   hotkey — гаряча клавіша — global keyboard shortcut
###   region — ділянка — a rectangle of the screen chosen by dragging
###   display — монітор — a physical screen
###   canvas — полотно — the drawing area of the editor
###   recording — запис — screen video (v2)
###   tray / menu bar — трей — notification area on Windows, menu bar on macOS


## App
## Product name and identity. "Znimok" is a name: never translate or transliterate it.


# @where: Everywhere the product is named: window title, tray, About
# @kind: label
# @max: 12
# @note: Brand name. Keep "Znimok" in every language.
app-name = Znimok

# @where: About page and installer, under the name
# @kind: hint
# @max: 60
app-tagline = Знімки й записи екрана з позначками

## Common
## Words shared by many screens. Use these ids instead of copying the same text.


# @where: Dialogs and modes: leave without applying
# @kind: button
# @max: 16
common-cancel = Скасувати

# @where: Icon-only close buttons (×) of dialogs, pills, tabs
# @kind: a11y
# @max: 24
common-close = Закрити

# @where: Crop mode, overlay editor: finish and apply
# @kind: button
# @max: 16
common-done = Готово

# @where: Assistant plan, dialogs: apply the change
# @kind: button
# @max: 16
common-apply = Застосувати

# @where: Crop, tone and similar: back to the original value
# @kind: button
# @max: 16
common-reset = Скинути

# @where: Save to the library (Ctrl+S); also confirm-on-close dialog
# @kind: button
# @max: 16
common-save = Зберегти

# @where: Export dialogs: opens the system save dialog
# @kind: button
# @max: 18
common-save-ellipsis = Зберегти…

# @where: Close-with-unsaved dialog, the quiet destructive choice
# @kind: button
# @max: 18
common-dont-save = Не зберігати

# @where: Context menus: delete the selected item
# @kind: menu
# @max: 20
common-delete = Видалити

# @where: Context menus: rename (F2)
# @kind: menu
# @max: 20
common-rename = Перейменувати

# @where: Context menus: duplicate (Ctrl+D)
# @kind: menu
# @max: 20
common-duplicate = Дублювати

# @where: Title bar menu button, file lists
# @kind: button
# @max: 16
common-open = Відкрити

# @where: Library home: open a file from disk
# @kind: button
# @max: 18
common-open-ellipsis = Відкрити…

# @where: Toasts: reveal the result (file, item)
# @kind: button
# @max: 16
common-show = Показати

# @where: Context menu of a mark: exclude from export (keeps it in the document)
# @kind: menu
# @max: 20
common-hide = Сховати

# @where: Icon-only "…" buttons that open more actions
# @kind: a11y
# @max: 24
common-more = Ще

# @where: Onboarding, banners: postpone
# @kind: button
# @max: 16
common-later = Пізніше

# @where: Onboarding: next step
# @kind: button
# @max: 16
common-next = Далі

# @where: Settings and onboarding: choose another folder
# @kind: button
# @max: 16
common-change-ellipsis = Змінити…

# @where: Inspector: add fill / effect / tag
# @kind: button
# @max: 16
common-add = Додати

# @where: Settings: add a share target, a key
# @kind: button
# @max: 16
common-add-ellipsis = Додати…

# @where: Settings row action that opens the page with details
# @kind: button
# @max: 16
common-manage = Керувати

# @where: Errors about permissions: re-test after the user changed settings
# @kind: button
# @max: 20
common-check-again = Перевірити ще раз

# @where: Segmented controls and pickers: nothing selected (effect, arrowhead, outline)
# @kind: option
# @max: 12
common-none = Немає

# @where: Tooltip of any control that is shown but not implemented yet (stable layout rule)
# @kind: tooltip
# @max: 40
# @note: Shown on greyed-out controls so their place does not change between releases.
common-in-development = В розробці

# @where: Three-way segmented control Never / Ask / Always
# @kind: option
# @max: 10
common-never = Ніколи

# @where: Three-way segmented control Never / Ask / Always
# @kind: option
# @max: 10
common-ask = Питати

# @where: Three-way segmented control Never / Ask / Always
# @kind: option
# @max: 10
common-always = Завжди

# @where: Short value "switched off" in segmented controls
# @kind: option
# @max: 10
common-off = вимк.

# @where: Relative time in logs: just now
# @kind: status
# @max: 14
common-now = зараз

# @where: Relative time in logs and library groups
# @kind: status
# @max: 14
common-yesterday = вчора

# @where: Library: group header of today's items
# @kind: heading
# @max: 14
common-today = Сьогодні

# @where: Library: group header of yesterday's items
# @kind: heading
# @max: 14
common-yesterday-heading = Учора

# @where: Size shown as width × height; keep the × sign
# @kind: label
# @max: 24
# @note: × is U+00D7 with spaces around, same in every language.
common-size-by = { $width } × { $height }

# @where: Estimated file size, e.g. "≈ 640 KB"; $size is already formatted with its unit
# @kind: label
# @max: 16
common-size-approx = ≈ { $size }

# @where: Number of marks (annotations) on a document; status bar, cards, pills
# @kind: label
# @max: 24
# @note: "Annotation" in English, "позначка" in Ukrainian — see the glossary.
common-marks =
    { $count ->
        [one] { $count } позначка
        [few] { $count } позначки
        [many] { $count } позначок
       *[other] { $count } позначки
    }

## Navigation
## The left rail of the main window and the way back to the library.


# @where: Left rail: Library page (also its accessible name)
# @kind: tooltip
# @max: 24
nav-library = Бібліотека

# @where: Left rail: Agents page
# @kind: tooltip
# @max: 24
nav-agents = Агенти

# @where: Left rail: Settings page
# @kind: tooltip
# @max: 24
nav-settings = Налаштування

# @where: Editor title bar: back arrow to the library
# @kind: a11y
# @max: 32
nav-back-to-library = До бібліотеки

## Editor title bar
## The row at the top of the editor window: document kind, name, save state, main actions.


# @where: Title bar chip before the document name
# @kind: badge
# @max: 12
doc-kind-screenshot = Знімок

# @where: Title bar chip before the document name
# @kind: badge
# @max: 12
doc-kind-video = Відео

# @where: Title bar chip for an empty canvas
# @kind: badge
# @max: 16
doc-kind-blank = Порожнє полотно

# @where: Title bar toggle; global setting, on by default
# @kind: label
# @max: 24
doc-autosave = Зберігати автоматично

# @where: Title bar, after the toggle: everything is in the library
# @kind: status
# @max: 16
doc-state-saved = збережено

# @where: Title bar: there are changes not in the library yet (auto-save off)
# @kind: status
# @max: 16
doc-state-unsaved = незбережено

# @where: Title bar: saving right now
# @kind: status
# @max: 16
doc-state-saving = збереження…

# @where: Title bar undo button; shortcut shown by the tooltip system
# @kind: tooltip
# @max: 28
doc-undo = Скасувати дію

# @where: Title bar redo button
# @kind: tooltip
# @max: 28
doc-redo = Повторити

# @where: Title bar: give the document to an AI agent (file + context)
# @kind: button
# @max: 20
doc-hand-to-agent = Передати агенту

# @where: Title bar main button: copy the image to the clipboard
# @kind: button
# @max: 16
# @note: Main action everywhere. Never call it "Export".
doc-copy = Копіювати

# @where: Arrow next to Copy: other ways to get the result out
# @kind: tooltip
# @max: 24
doc-other-ways = Інші способи

# @where: Name of a new document before it is saved; $date and $time are preformatted
# @kind: label
# @max: 40
doc-untitled = Знімок { $date } { $time }

## Open menu
## The "Open ▾" menu in the title bar and on the home screen.


# @where: Open ▾ menu; shortcut Ctrl+O / ⌘O shown by the menu
# @kind: menu
# @max: 32
open-file = Файл…

# @where: Open ▾ menu: image from the clipboard
# @kind: menu
# @max: 32
open-clipboard = З буфера обміну

# @where: Open ▾ menu: submenu with capture kinds
# @kind: menu
# @max: 32
open-new-shot = Новий знімок

# @where: Open ▾ menu: empty canvas
# @kind: menu
# @max: 32
open-blank = Порожнє полотно

# @where: Toast when "From clipboard" finds no image
# @kind: error
# @max: 80
open-error-no-image = У буфері обміну немає зображення.

# @where: Toast when a dropped or opened file cannot be read as an image
# @kind: error
# @max: 120
open-error-not-image = Не вдалося відкрити «{ $name }»: це не зображення, яке розуміє Znimok.

## Other ways menu
## The menu behind the arrow next to Copy.


# @where: First item, same as the main Copy button
# @kind: menu
# @max: 32
share-copy-image = Копіювати зображення

# @where: Grey note after the first item
# @kind: hint
# @max: 24
share-copy-image-hint = головна кнопка

# @where: Puts the file itself on the clipboard (paste into a chat or a folder)
# @kind: menu
# @max: 32
share-copy-file = Копіювати як файл

# @where: Opens the OS share sheet
# @kind: menu
# @max: 36
share-system = Поділитися…

# @where: Same as share-system where the menu names the OS feature
# @kind: menu
# @max: 40
share-system-long = Системне меню «Поділитися»…

# @where: Opens the export dialog (Ctrl+Shift+S)
# @kind: menu
# @max: 32
share-export = Експортувати файл…

# @where: Single HTML file with marks that opens anywhere
# @kind: menu
# @max: 36
share-html = Самодостатній HTML…

# @where: Group heading for integrations
# @kind: heading
# @max: 24
share-targets = Цілі поширення

# @where: Badge next to the heading: not available yet
# @kind: badge
# @max: 12
share-targets-later = згодом

# @where: Adds an integration
# @kind: menu
# @max: 24
share-add-target = Додати ціль…

## Tools
## The vertical tool rail of the editor and the overlay. Tooltips carry the one-key shortcut in parentheses; keep the Latin letter as is — it is a physical key, not a word. Names without the key are used as headings.


# @where: Tool rail button tooltip and accessible name; (V) is its shortcut
# @kind: tooltip
# @max: 28
tool-select = Вибір (V)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-select-name = Вибір

# @where: Tool rail button tooltip and accessible name; (R) is its shortcut
# @kind: tooltip
# @max: 28
tool-rect = Прямокутник (R)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-rect-name = Прямокутник

# @where: Tool rail button tooltip and accessible name; (E) is its shortcut
# @kind: tooltip
# @max: 28
tool-ellipse = Еліпс (E)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-ellipse-name = Еліпс

# @where: Tool rail button tooltip and accessible name; (L) is its shortcut
# @kind: tooltip
# @max: 28
tool-arrow = Стрілка (L)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-arrow-name = Стрілка

# @where: Tool rail button tooltip and accessible name; (P) is its shortcut
# @kind: tooltip
# @max: 28
tool-pen = Олівець (P)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-pen-name = Олівець

# @where: Tool rail button tooltip and accessible name; (T) is its shortcut
# @kind: tooltip
# @max: 28
tool-text = Напис (T)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-text-name = Напис

# @where: Tool rail button tooltip and accessible name; (B) is its shortcut
# @kind: tooltip
# @max: 28
tool-hide = Приховати (B)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-hide-name = Приховати

# @where: Tool rail button tooltip and accessible name; (H) is its shortcut
# @kind: tooltip
# @max: 28
tool-highlighter = Маркер (H)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-highlighter-name = Маркер

# @where: Tool rail button tooltip and accessible name; (N) is its shortcut
# @kind: tooltip
# @max: 28
tool-counter = Лічильник (N)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-counter-name = Лічильник

# @where: Tool rail button tooltip and accessible name; (S) is its shortcut
# @kind: tooltip
# @max: 28
tool-stamp = Штамп (S)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-stamp-name = Штамп

# @where: Tool rail button tooltip and accessible name; (I) is its shortcut
# @kind: tooltip
# @max: 28
tool-image = Зображення (I)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-image-name = Зображення

# @where: Tool rail button tooltip and accessible name; (C) is its shortcut
# @kind: tooltip
# @max: 28
tool-crop = Кадрувати (C)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-crop-name = Кадрувати

# @where: Mark kind name: a straight line (arrow with no heads)
# @kind: label
# @max: 20
tool-line-name = Лінія

# @where: Video editor rail: split the clip at the playhead; (S) is its shortcut
# @kind: tooltip
# @max: 28
tool-cut = Розрізати (S)

# @where: Narrow windows: the rest of the tools behind "…"
# @kind: tooltip
# @max: 24
tool-more = Інші інструменти

## Context bar
## The floating bar next to the selected mark (window, overlay and video share it).


# @where: Colour swatch button for outlines
# @kind: a11y
# @max: 32
ctx-stroke-colour = Колір контуру

# @where: Colour swatch button for fills, with a colour set
# @kind: a11y
# @max: 32
ctx-fill-colour = Колір заливки

# @where: Colour swatch button for fills when there is no fill
# @kind: a11y
# @max: 32
ctx-fill-none = Колір заливки: немає

# @where: Text mark: colour of the letters
# @kind: a11y
# @max: 32
ctx-text-colour = Колір тексту

# @where: Text mark: colour of the outline around the letters
# @kind: a11y
# @max: 32
ctx-text-outline = Колір обводки

# @where: Line width option
# @kind: a11y
# @max: 16
ctx-thin = Тонка

# @where: Line width option
# @kind: a11y
# @max: 16
ctx-medium = Середня

# @where: Line width option
# @kind: a11y
# @max: 16
ctx-thick = Товста

# @where: Corner rounding button
# @kind: a11y
# @max: 16
ctx-corners = Кути

# @where: Solid / dashed button
# @kind: a11y
# @max: 24
ctx-line-style = Стиль лінії

# @where: Shadow / glow button
# @kind: a11y
# @max: 16
ctx-effect = Ефект

# @where: Text mark
# @kind: a11y
# @max: 16
ctx-bold = Жирний

# @where: Text mark
# @kind: a11y
# @max: 16
ctx-italic = Курсив

# @where: Text alignment
# @kind: a11y
# @max: 24
ctx-align-left = Ліворуч

# @where: Text alignment
# @kind: a11y
# @max: 24
ctx-align-centre = По центру

# @where: Text alignment
# @kind: a11y
# @max: 24
ctx-align-right = Праворуч

# @where: Status hint while drawing a rectangle/ellipse
# @kind: hint
# @max: 32
# @note: "Shift" is a key name, keep it.
ctx-hint-square = Shift — квадрат

# @where: Status hint on the canvas
# @kind: hint
# @max: 32
ctx-hint-zoom = Alt + коліщатко — зум

## Colour picker
## The popover that opens from a colour swatch.


# @where: Accessible name of the popover
# @kind: a11y
# @max: 24
colour-picker = Вибір кольору

# @where: Button that picks a colour from the screenshot or the screen
# @kind: button
# @max: 16
colour-eyedropper = Піпетка

# @where: Tooltip of the eyedropper button
# @kind: tooltip
# @max: 72
colour-eyedropper-tip = Піпетка: узяти колір зі знімка або з екрана

# @where: Label of the hex code field; keep "HEX"
# @kind: label
# @max: 6
colour-hex = HEX

# @where: Opacity field (0–100 %)
# @kind: a11y
# @max: 16
colour-opacity = Прозорість

# @where: Row of recently used colours
# @kind: heading
# @max: 16
colour-recent = Останні

# @where: Adds the colour to the user's palette
# @kind: button
# @max: 24
colour-save = Зберегти в палітру

# @where: Grey note at the bottom of the popover
# @kind: hint
# @max: 160
colour-eyedropper-note = Піпетка бере колір зі знімка, а з живого екрана — через захоплення (Windows) або системний семплер (macOS).

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-red = Червоний

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-orange = Помаранчевий

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-yellow = Жовтий

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-green = Зелений

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-blue = Синій

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-purple = Фіолетовий

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-white = Білий

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-black = Чорний

## Inspector
## The right panel of the editor window: tabs and the Object tab.


# @where: Inspector tab: the selected mark
# @kind: tab
# @max: 10
insp-tab-object = Об'єкт

# @where: Inspector tab: list of marks, z-order
# @kind: tab
# @max: 10
insp-tab-layers = Шари

# @where: Inspector tab: the screenshot itself (crop, rotation, tone)
# @kind: tab
# @max: 10
insp-tab-shot = Знімок

# @where: Inspector tab: title, author, tags…
# @kind: tab
# @max: 10
insp-tab-meta = Мета

# @where: Video inspector tab: trim and sound
# @kind: tab
# @max: 10
insp-tab-clip = Кліп

# @where: Video inspector tab: DevTools log and clicks
# @kind: tab
# @max: 10
insp-tab-events = Події

# @where: Collapse the inspector
# @kind: a11y
# @max: 24
insp-hide-panel = Сховати панель

# @where: Expand the inspector
# @kind: a11y
# @max: 24
insp-show-panel = Показати панель

# @where: After the kind name in the Object tab: which mark of how many
# @kind: label
# @max: 24
insp-selected-of = { $index } з { $total }

# @where: Object tab when nothing is selected
# @kind: hint
# @max: 80
insp-nothing-selected = Виберіть позначку, щоб змінити її, або інструмент ліворуч.

# @where: Object tab of a screenshot without marks
# @kind: hint
# @max: 100
insp-no-marks = Знімок ще без позначок. Виберіть інструмент ліворуч і малюйте на знімку.

# @where: Section: outline colour and width
# @kind: label
# @max: 16
insp-stroke = Контур

# @where: Section: fill colour
# @kind: label
# @max: 16
insp-fill = Заливка

# @where: Section: corner rounding
# @kind: label
# @max: 16
insp-corners = Кути

# @where: Corner option
# @kind: option
# @max: 10
insp-corners-sharp = Гострі

# @where: Corner option
# @kind: option
# @max: 10
insp-corners-soft = М'які

# @where: Corner option
# @kind: option
# @max: 10
insp-corners-round = Круглі

# @where: Section: opacity slider
# @kind: label
# @max: 16
insp-opacity = Прозорість

# @where: Section: list of effects
# @kind: label
# @max: 16
insp-effects = Ефекти

# @where: Effects section when empty
# @kind: hint
# @max: 24
insp-effects-none = Немає ефектів

# @where: Effect name (list row and Add menu)
# @kind: menu
# @max: 16
insp-effect-shadow = Тінь

# @where: Effect name
# @kind: menu
# @max: 16
insp-effect-glow = Свічення

# @where: Effect name
# @kind: menu
# @max: 16
insp-effect-outline = Обводка

# @where: Toggle of an effect row; $name is the effect name
# @kind: a11y
# @max: 32
insp-effect-on = { $name }: увімкнено

# @where: Toggle of an effect row
# @kind: a11y
# @max: 32
insp-effect-off = { $name }: вимкнено

# @where: Chevron that opens an effect's settings
# @kind: a11y
# @max: 16
insp-expand = Розгорнути

# @where: Chevron that closes an effect's settings
# @kind: a11y
# @max: 16
insp-collapse = Згорнути

# @where: Shadow setting (short label before a number field)
# @kind: label
# @max: 10
insp-shadow-offset = Зсув

# @where: Shadow setting (short label)
# @kind: label
# @max: 10
# @note: Very short label; the field's accessible name is insp-shadow-blur-long.
insp-shadow-blur = Розм.

# @where: Accessible name of the shadow blur field
# @kind: a11y
# @max: 24
insp-shadow-blur-long = Розмиття

# @where: Accessible name of the shadow opacity field
# @kind: a11y
# @max: 24
insp-shadow-opacity = Прозорість тіні

# @where: Design note for developers is not shown; this is the hint under the list
# @kind: hint
# @max: 160
insp-effects-note = Ефекти додаються, вмикаються й упорядковуються.

# @where: Position field label; keep the Latin letter
# @kind: label
# @max: 2
insp-x = X

# @where: Position field label; keep the Latin letter
# @kind: label
# @max: 2
insp-y = Y

# @where: Width field label, one letter
# @kind: label
# @max: 2
insp-w = Ш

# @where: Height field label, one letter
# @kind: label
# @max: 2
insp-h = В

# @where: Accessible name of the width field
# @kind: a11y
# @max: 16
insp-width = Ширина

# @where: Accessible name of the height field
# @kind: a11y
# @max: 16
insp-height = Висота

# @where: Rotation field (degrees)
# @kind: label
# @max: 16
insp-rotation = Поворот

# @where: Line style option
# @kind: a11y
# @max: 20
insp-line-solid = Суцільна

# @where: Line style option
# @kind: a11y
# @max: 20
insp-line-dashed = Пунктир

# @where: Line style option
# @kind: a11y
# @max: 20
insp-line-dashdot = Штрихпунктир

# @where: Arrow / line / pen section: arrowheads
# @kind: label
# @max: 20
insp-heads = Наконечники

# @where: Grey note after the heading: heads also work for the pen
# @kind: hint
# @max: 24
insp-heads-pen = і для олівця

# @where: Arrowhead at the start of the line
# @kind: label
# @max: 10
insp-head-start = Початок

# @where: Arrowhead at the end of the line
# @kind: label
# @max: 10
insp-head-end = Кінець

# @where: Arrowhead size (S / M / L)
# @kind: label
# @max: 10
insp-head-size = Розмір

# @where: Arrowhead shape
# @kind: a11y
# @max: 16
insp-head-triangle = Трикутник

# @where: Arrowhead shape: open V
# @kind: a11y
# @max: 16
insp-head-chevron = Пташка

# @where: Arrowhead shape
# @kind: a11y
# @max: 16
insp-head-dot = Кружок

# @where: Size option, one letter; keep S/M/L in every language
# @kind: option
# @max: 2
insp-size-s = S

# @where: Size option
# @kind: option
# @max: 2
insp-size-m = M

# @where: Size option
# @kind: option
# @max: 2
insp-size-l = L

## Text mark
## Inspector and hints while a text mark is edited.


# @where: After the kind name in the inspector while typing
# @kind: status
# @max: 16
text-editing = редагується

# @where: Section: typeface
# @kind: label
# @max: 12
text-font = Шрифт

# @where: Font size field
# @kind: label
# @max: 12
text-size = Кегль

# @where: Section: text block width
# @kind: label
# @max: 12
text-block = Блок

# @where: Accessible name of the block width field
# @kind: a11y
# @max: 24
text-block-width = Ширина блока

# @where: Under the block width
# @kind: hint
# @max: 100
text-block-hint = Ширина 0 — без переносів; ширину змінюють ручки з боків.

# @where: Section: outline around letters
# @kind: label
# @max: 12
text-outline = Обводка

# @where: Status hint: Enter finishes
# @kind: hint
# @max: 16
text-hint-done = готово

# @where: Status hint: Shift+Enter adds a line
# @kind: hint
# @max: 16
text-hint-newline = новий рядок

# @where: Status hint: Esc cancels
# @kind: hint
# @max: 16
text-hint-cancel = скасувати

# @where: Status bar while a text mark is edited
# @kind: status
# @max: 24
text-status = редагування напису

# @where: Button: smaller type
# @kind: tooltip
# @max: 24
text-size-down = Менший кегль

# @where: Button: larger type
# @kind: tooltip
# @max: 24
text-size-up = Більший кегль

## Hide, highlighter, counter, stamp
## Mark-specific controls.


# @where: Hide style: blur what is under the mark
# @kind: option
# @max: 12
hide-blur = Розмиття

# @where: Hide style: pixelate
# @kind: option
# @max: 12
hide-pixels = Пікселі

# @where: Hide style: solid plate
# @kind: option
# @max: 12
hide-plate = Плашка

# @where: Slider: how strong the hiding is
# @kind: label
# @max: 16
hide-strength = Сила

# @where: Layer row of a hide mark with a detected kind; $what is e.g. "e-mail"
# @kind: label
# @max: 40
hide-hidden-name = Приховано: { $what }

# @where: Highlighter: band height
# @kind: label
# @max: 16
mark-band = Висота смуги

# @where: Counter: badge shape
# @kind: label
# @max: 16
counter-shape = Форма

# @where: Counter shape
# @kind: option
# @max: 12
counter-circle = Кружок

# @where: Counter shape
# @kind: option
# @max: 12
counter-square = Квадрат

# @where: Counter shape: map-pin
# @kind: option
# @max: 12
counter-pin = Шпилька

# @where: Counter: colour of the number
# @kind: label
# @max: 16
counter-digit-colour = Цифра

# @where: Counter number colour option
# @kind: tooltip
# @max: 60
counter-digit-auto = Авто: чорна чи біла — що краще читається

# @where: Counter context menu
# @kind: menu
# @max: 32
counter-start-from = Почати нумерацію з…

# @where: Counter context menu
# @kind: menu
# @max: 32
counter-new-group = Нова група нумерації

# @where: Counter context menu
# @kind: menu
# @max: 48
counter-edit-group = Редагувати всю групу (колір, розмір, форму)

# @where: Counter context menu
# @kind: menu
# @max: 32
counter-delete-group = Видалити групу

# @where: Counter inspector: the number the next click will place
# @kind: hint
# @max: 16
counter-next = Далі: { $n }

# @where: Layer row name of a counter
# @kind: label
# @max: 20
counter-name = Лічильник { $n }

# @where: Stamp and emoji picker (tool S)
# @kind: heading
# @max: 40
stamp-picker = Штампи й емодзі

# @where: Search field of the picker
# @kind: placeholder
# @max: 24
stamp-search = Пошук емодзі

# @where: Picker tab with Znimok stamps
# @kind: tab
# @max: 12
stamp-stamps = Штампи

# @where: Picker section
# @kind: tab
# @max: 24
stamp-emoji-recent = Емодзі · нещодавні

# @where: Stamp name (accessible name)
# @kind: a11y
# @max: 20
stamp-check = Галочка

# @where: Stamp name (accessible name)
# @kind: a11y
# @max: 20
stamp-cross = Хрестик

# @where: Stamp name (accessible name)
# @kind: a11y
# @max: 20
stamp-question = Питання

# @where: Stamp name (accessible name)
# @kind: a11y
# @max: 20
stamp-exclamation = Знак оклику

# @where: Stamp name (accessible name)
# @kind: a11y
# @max: 20
stamp-star = Зірка

# @where: Stamp name (accessible name)
# @kind: a11y
# @max: 20
stamp-warning = Попередження

## Layers
## The Layers tab of the inspector.


# @where: Heading of the list; order is top to bottom
# @kind: label
# @max: 40
layers-count =
    { $count ->
        [one] { $count } позначка · зверху вниз
        [few] { $count } позначки · зверху вниз
        [many] { $count } позначок · зверху вниз
       *[other] { $count } позначки · зверху вниз
    }

# @where: Button: group the selected marks
# @kind: a11y
# @max: 20
layers-group = Групувати

# @where: Button: ungroup
# @kind: a11y
# @max: 20
layers-ungroup = Розгрупувати

# @where: Button: one step up in z-order
# @kind: a11y
# @max: 20
layers-up = Вище

# @where: Button: one step down in z-order
# @kind: a11y
# @max: 20
layers-down = Нижче

# @where: Eye button of a hidden row
# @kind: a11y
# @max: 20
layers-show = Показати

# @where: Eye button of a visible row
# @kind: a11y
# @max: 20
layers-hide = Сховати

# @where: Chevron of a group row
# @kind: a11y
# @max: 24
layers-collapse-group = Згорнути групу

# @where: Chevron of a group row
# @kind: a11y
# @max: 24
layers-expand-group = Розгорнути групу

# @where: Row of a named group
# @kind: label
# @max: 40
layers-group-name = Група «{ $name }»

# @where: Default name of a new group
# @kind: label
# @max: 24
layers-group-default = Група { $n }

# @where: Last row: the screenshot itself
# @kind: label
# @max: 48
layers-background = Знімок (тло) — завжди внизу

# @where: Grey note under the list
# @kind: hint
# @max: 160
layers-hint = Тягніть, щоб змінити порядок; клік вибирає, F2 — перейменувати; група — один рядок, її учасники поруч.

# @where: Layers tab with no marks
# @kind: hint
# @max: 60
layers-empty = Позначок ще немає — намалюйте щось на полотні.

## Arrange
## Align, distribute and z-order actions (context bar with several marks, menus).


# @where: Several marks selected
# @kind: tooltip
# @max: 32
arrange-align-left = Вирівняти ліві краї

# @where: Several marks selected
# @kind: tooltip
# @max: 32
arrange-align-hcentre = Вирівняти центри по вертикалі

# @where: Several marks selected
# @kind: tooltip
# @max: 32
arrange-align-right = Вирівняти праві краї

# @where: Several marks selected
# @kind: tooltip
# @max: 32
arrange-align-top = Вирівняти верхні краї

# @where: Several marks selected
# @kind: tooltip
# @max: 32
arrange-align-vcentre = Вирівняти центри по горизонталі

# @where: Several marks selected
# @kind: tooltip
# @max: 32
arrange-align-bottom = Вирівняти нижні краї

# @where: Three or more selected
# @kind: tooltip
# @max: 32
arrange-distribute-x = Рівні проміжки по ширині

# @where: Three or more selected
# @kind: tooltip
# @max: 32
arrange-distribute-y = Рівні проміжки по висоті

# @where: Context menu; shortcut ]
# @kind: menu
# @max: 28
arrange-front = На передній план

# @where: Context menu; shortcut [
# @kind: menu
# @max: 28
arrange-back = На задній план

# @where: Context menu; Ctrl+G
# @kind: menu
# @max: 20
arrange-group = Групувати

# @where: Context menu; Ctrl+Shift+G
# @kind: menu
# @max: 20
arrange-ungroup = Розгрупувати

# @where: Context menu: new marks of this kind use this style
# @kind: menu
# @max: 40
arrange-default-style = Зробити стилем за замовчуванням

# @where: Status bar with several marks selected
# @kind: status
# @max: 24
arrange-selected = Вибрано: { $count }

## Image tab
## The Image tab of the inspector: source, crop, rotation, tone, image and canvas size.


# @where: Source line: window capture; $title is the window title
# @kind: label
# @max: 48
shot-source-window = Вікно «{ $title }»

# @where: Source line: whole display; $n is its number
# @kind: label
# @max: 48
shot-source-display = Монітор { $n }

# @where: Source line: region capture
# @kind: label
# @max: 48
shot-source-region = Ділянка

# @where: Source line: pasted image
# @kind: label
# @max: 48
shot-source-clipboard = З буфера обміну

# @where: Source line: opened file
# @kind: label
# @max: 48
shot-source-file = Файл «{ $name }»

# @where: Facts line: an HDR capture tone-mapped to SDR; $nits is a number
# @kind: label
# @max: 48
shot-hdr = HDR → SDR, біле { $nits } ніт

# @where: Facts line: marks left outside the crop (they are kept)
# @kind: label
# @max: 40
shot-outside =
    { $count ->
        [one] { $count } поза кадром
        [few] { $count } поза кадром
        [many] { $count } поза кадром
       *[other] { $count } поза кадром
    }

# @where: Section: crop
# @kind: label
# @max: 12
shot-crop = Кадр

# @where: Crop aspect: free
# @kind: option
# @max: 12
shot-crop-free = Вільно

# @where: Starts crop mode; (C) is the shortcut
# @kind: button
# @max: 20
shot-crop-button = Кадрувати (C)

# @where: Section: rotate and mirror
# @kind: label
# @max: 28
shot-rotate-mirror = Поворот і дзеркало

# @where: Button
# @kind: a11y
# @max: 24
shot-rotate-left = Повернути ліворуч

# @where: Button
# @kind: a11y
# @max: 24
shot-rotate-right = Повернути праворуч

# @where: Button
# @kind: a11y
# @max: 32
shot-mirror-h = Дзеркало по горизонталі

# @where: Button
# @kind: a11y
# @max: 32
shot-mirror-v = Дзеркало по вертикалі

# @where: Section: tone
# @kind: label
# @max: 12
shot-tone = Тон

# @where: Hold to see the original
# @kind: button
# @max: 16
shot-compare = Порівняти

# @where: Tooltip of Compare
# @kind: tooltip
# @max: 60
shot-compare-tip = Тримайте, щоб побачити оригінал

# @where: Tone slider; value in EV
# @kind: label
# @max: 14
shot-exposure = Експозиція

# @where: Tone slider: midtones
# @kind: label
# @max: 14
shot-gamma = Гамма

# @where: Tone slider
# @kind: label
# @max: 14
shot-contrast = Контраст

# @where: Reset button of the tone section
# @kind: tooltip
# @max: 60
shot-tone-reset-tip = Повернути тон, як було при захопленні

# @where: Grey note under the tone sliders
# @kind: hint
# @max: 120
shot-tone-hint = Рецепт поверх оригіналу: оригінал не змінюється, «Порівняти» показує його, поки тримаєш.

# @where: Opens the image size dialog
# @kind: button
# @max: 20
shot-image-size = Зображення…

# @where: Opens the canvas size dialog
# @kind: button
# @max: 20
shot-canvas-size = Полотно…

# @where: Grey note under the two buttons
# @kind: hint
# @max: 120
shot-bake-hint = Обидва «запікають» рецепт у новий оригінал; попередній лишається для скасування.

# @where: Dialog title
# @kind: title
# @max: 32
size-image-title = Розмір зображення

# @where: Dialog title
# @kind: title
# @max: 32
size-canvas-title = Розмір полотна

# @where: Dialog: current size
# @kind: label
# @max: 32
size-now = Зараз: { $width } × { $height }

# @where: Dialog field
# @kind: label
# @max: 12
size-percent = Відсоток

# @where: Dialog checkbox
# @kind: label
# @max: 32
size-keep-ratio = Зберігати пропорції

# @where: Dialog checkbox (image size only)
# @kind: label
# @max: 48
size-scale-text = Масштабувати текст разом із зображенням

# @where: Dialog checkbox: nearest-neighbour resampling
# @kind: label
# @max: 40
size-sharp = Різко (без згладжування)

# @where: Dialog main button
# @kind: button
# @max: 16
size-apply = Змінити розмір

# @where: Dialog note (image size)
# @kind: hint
# @max: 160
size-note-image = Товщина ліній, кружечки й штампи лишаються тими самими; текст масштабується лише з прапорцем вище.

# @where: Dialog note (canvas size)
# @kind: hint
# @max: 160
size-note-canvas = Знімок стане окремим об'єктом на більшому чи меншому полотні; нічого не розтягується.

## Meta tab
## Title, description, author and the rest of the document metadata.


# @where: Field
# @kind: label
# @max: 16
meta-title = Назва

# @where: Field
# @kind: label
# @max: 16
meta-description = Опис

# @where: Field
# @kind: label
# @max: 16
meta-author = Автор

# @where: Field: copyright notice
# @kind: label
# @max: 16
meta-rights = Права

# @where: Field
# @kind: label
# @max: 16
meta-tags = Теги

# @where: Placeholder of the new-tag field
# @kind: placeholder
# @max: 16
meta-add-tag = додати…

# @where: Accessible name of the new-tag field
# @kind: a11y
# @max: 16
meta-add-tag-a11y = Додати тег

# @where: Field: capture date and time
# @kind: label
# @max: 20
meta-taken = Дата зйомки

# @where: Field: where it came from
# @kind: label
# @max: 16
meta-source = Джерело

# @where: Section: what export does with metadata
# @kind: label
# @max: 20
meta-on-export = При експорті

# @where: Checkbox
# @kind: label
# @max: 32
meta-write = Записувати в PNG / JPEG

# @where: Checkbox: remove everything
# @kind: label
# @max: 32
meta-strip = Прибрати всі метадані

# @where: Grey note
# @kind: hint
# @max: 120
meta-program-note = Поле «Програма» = Znimok і версія, крім режиму «прибрати все».

## Crop mode
## The editor while the crop frame is being edited.


# @where: Crop bar label
# @kind: label
# @max: 12
crop-title = Кадр

# @where: Lock icon next to the proportions
# @kind: a11y
# @max: 32
crop-lock-ratio = Зафіксувати пропорції

# @where: Status hint after "Enter"
# @kind: hint
# @max: 16
crop-hint-done = готово

# @where: Status hint after "Esc"
# @kind: hint
# @max: 16
crop-hint-cancel = скасувати

# @where: Status hint
# @kind: hint
# @max: 40
crop-hint-move = тягніть усередині — зсув кадру

# @where: Frame width field
# @kind: a11y
# @max: 24
crop-width = Ширина кадру

# @where: Frame height field
# @kind: a11y
# @max: 24
crop-height = Висота кадру

# @where: Inspector line: original size → frame size
# @kind: label
# @max: 48
crop-sizes = { $width } × { $height } → кадр { $cw } × { $ch }

# @where: Inspector line: marks outside the new frame are kept
# @kind: hint
# @max: 60
crop-outside-kept =
    { $count ->
        [one] { $count } позначка поза кадром — вона збережеться
        [few] { $count } позначки поза кадром — вони збережуться
        [many] { $count } позначок поза кадром — вони збережуться
       *[other] { $count } позначки поза кадром — вони збережуться
    }

# @where: Grey note while cropping
# @kind: hint
# @max: 160
crop-dimmed-note = Решта інспектора приглушена, поки триває кадрування. Кадр — властивість документа: позначки поза ним не видаляються.

# @where: Status bar while cropping
# @kind: status
# @max: 48
crop-status =
    { $count ->
        [one] кадрування · { $count } позначка поза кадром
        [few] кадрування · { $count } позначки поза кадром
        [many] кадрування · { $count } позначок поза кадром
       *[other] кадрування · { $count } позначки поза кадром
    }

## Canvas and status bar
## Context menu of the canvas, drag-and-drop, status bar.


# @where: Context menu; Ctrl+V
# @kind: menu
# @max: 24
canvas-paste = Вставити

# @where: Context menu; Ctrl+A
# @kind: menu
# @max: 24
canvas-select-all = Вибрати все

# @where: Context menu; Ctrl+0
# @kind: menu
# @max: 24
canvas-fit = Вписати

# @where: Context menu; Ctrl+1
# @kind: menu
# @max: 24
canvas-zoom-100 = Масштаб 100 %

# @where: Drop overlay while a file is dragged over the canvas
# @kind: title
# @max: 48
canvas-drop-add = Відпустіть, щоб додати як позначку

# @where: Drop overlay second line; { $key } is "Shift"
# @kind: hint
# @max: 60
canvas-drop-shift = Утримуйте { $key }, щоб відкрити як новий знімок

# @where: Empty canvas
# @kind: hint
# @max: 100
canvas-blank-hint = Порожнє полотно. Ctrl+V — вставити зображення; перетягніть файл, щоб відкрити.

# @where: Status bar button
# @kind: a11y
# @max: 24
status-fit = Вписати у вікно

# @where: Status bar zoom value field
# @kind: a11y
# @max: 16
status-zoom = Масштаб

# @where: Status bar button
# @kind: a11y
# @max: 24
status-zoom-100 = Масштаб 100 %

# @where: Status bar: no crop
# @kind: status
# @max: 32
status-crop-whole = Кадр: увесь знімок

# @where: Status bar: current crop
# @kind: status
# @max: 32
status-crop-size = Кадр: { $width } × { $height }

# @where: Status bar after a save
# @kind: status
# @max: 32
status-saved-library = Збережено в бібліотеку

## Capture overlay
## The frozen-screen overlay for choosing what to capture. Key names (Shift, Alt, Space, Esc) are keys: keep them.


# @where: Hint chip: releasing the mouse without a key
# @kind: badge
# @max: 12
capture-release = відпустити

# @where: Hint: where the capture goes on release
# @kind: hint
# @max: 20
capture-to-editor = у редактор

# @where: Hint after "Shift"
# @kind: hint
# @max: 20
capture-to-clipboard = у буфер

# @where: Hint after "Alt"
# @kind: hint
# @max: 20
capture-over-screen = поверх екрану

# @where: Hint after "Space"
# @kind: hint
# @max: 20
capture-whole-screen = весь екран

# @where: Hint chip: a click (not a drag)
# @kind: badge
# @max: 12
capture-click = клік

# @where: Hint after "click"
# @kind: hint
# @max: 20
capture-window = вікно

# @where: Hint after "Esc"
# @kind: hint
# @max: 20
capture-cancel = скасувати

# @where: Mode switch, top right
# @kind: option
# @max: 12
capture-mode-shot = Знімок

# @where: Mode switch, top right
# @kind: option
# @max: 12
capture-mode-record = Запис

# @where: Countdown before a delayed capture: Esc cancels
# @kind: hint
# @max: 24
capture-countdown-cancel = Esc — скасувати

## Overlay editor
## Editing right over the frozen screen (Alt on release).


# @where: Chip at the left of the overlay bar
# @kind: badge
# @max: 20
overlay-chip = Поверх екрану

# @where: Button: move to the editor window
# @kind: a11y
# @max: 32
overlay-open-window = Відкрити у вікні редактора

# @where: Tooltip of the same button
# @kind: tooltip
# @max: 80
overlay-open-window-tip = У вікно редактора — тон, розмір, бібліотека

# @where: Hint in the overlay status
# @kind: hint
# @max: 48
overlay-hint-frame = тягніть кути рамки — це кадр

# @where: Hint after "Ctrl+S"
# @kind: hint
# @max: 20
overlay-hint-save = у бібліотеку

# @where: Hint after "Esc"
# @kind: hint
# @max: 16
overlay-hint-close = закрити

## Pill
## The small card in the corner of the display after a capture (6 s).


# @where: Pill title after a region capture
# @kind: title
# @max: 40
pill-region-copied = Знімок ділянки скопійовано

# @where: Pill title after a whole-screen capture
# @kind: title
# @max: 40
pill-screen-copied = Знімок екрана скопійовано

# @where: Pill title after a window capture
# @kind: title
# @max: 40
pill-window-copied = Знімок вікна скопійовано

# @where: Pill title when the capture only went to the library
# @kind: title
# @max: 40
pill-saved = Збережено в бібліотеку

# @where: Pill second line
# @kind: hint
# @max: 48
pill-where = { $width } × { $height } · у буфері й бібліотеці

# @where: Pill button
# @kind: button
# @max: 14
pill-edit = Редагувати

# @where: Pill "…" button
# @kind: a11y
# @max: 80
pill-more = Ще: зберегти як файл, передати агенту, показати в бібліотеці

# @where: Pill menu
# @kind: menu
# @max: 28
pill-save-file = Зберегти як файл…

# @where: Pill menu
# @kind: menu
# @max: 28
pill-show-library = Показати в бібліотеці

# @where: Yellow pill while an agent captures; $client is its name
# @kind: title
# @max: 48
pill-agent-capturing = { $client } знімає екран

## Tray
## The tray menu on Windows and the menu bar menu on macOS (same items, native menu).


# @where: Tray menu
# @kind: menu
# @max: 28
tray-region = Знімок ділянки

# @where: Tray menu
# @kind: menu
# @max: 28
tray-screen = Знімок екрана

# @where: Tray menu
# @kind: menu
# @max: 28
tray-record = Записати відео

# @where: Tray menu
# @kind: menu
# @max: 28
tray-open = Відкрити Znimok

# @where: Tray menu: release all hotkeys for a while
# @kind: menu
# @max: 32
tray-pause-keys = Призупинити гарячі клавіші

# @where: Tray menu while paused
# @kind: menu
# @max: 32
tray-resume-keys = Відновити гарячі клавіші

# @where: Tray menu (Windows)
# @kind: menu
# @max: 20
tray-quit = Вийти

# @where: Menu bar menu (macOS)
# @kind: menu
# @max: 24
tray-quit-mac = Вийти з Znimok

# @where: Tray icon tooltip when idle
# @kind: tooltip
# @max: 40
tray-tooltip = Znimok

# @where: Tray icon tooltip while recording; $time like 00:12
# @kind: tooltip
# @max: 40
tray-tooltip-recording = Znimok — запис { $time }

# @where: Tray icon tooltip while hotkeys are paused
# @kind: tooltip
# @max: 40
tray-tooltip-paused-keys = Znimok — клавіші призупинено

## Export dialog
## Export a file (Ctrl+Shift+S).


# @where: Dialog title and accessible name
# @kind: title
# @max: 24
export-title = Експорт файлу

# @where: Left column heading
# @kind: label
# @max: 20
export-preview = Попередній перегляд

# @where: Fact label
# @kind: label
# @max: 12
export-size = Розмір

# @where: Fact label: estimated file size
# @kind: label
# @max: 12
export-file = Файл

# @where: Fact label: number of annotations
# @kind: label
# @max: 12
export-marks = Позначок

# @where: Note under the preview
# @kind: hint
# @max: 140
export-flat-note = Файл із позначками — плоске зображення. Проєкт із можливістю редагувати лишається в бібліотеці.

# @where: Field
# @kind: label
# @max: 12
export-format = Формат

# @where: Under the format choice
# @kind: hint
# @max: 120
export-format-hint = PNG — без втрат, з прозорістю. JPEG і WebP — менші файли для фото й чатів.

# @where: Field
# @kind: label
# @max: 12
export-scale = Масштаб

# @where: Field (JPEG/WebP)
# @kind: label
# @max: 12
export-quality = Якість

# @where: Checkbox
# @kind: label
# @max: 56
export-metadata = Записати метадані (назва, опис, автор, дата)

# @where: Checkbox
# @kind: label
# @max: 32
export-white-bg = Прозоре тло → біле

# @where: Checkbox
# @kind: label
# @max: 32
export-remember = Пам'ятати ці налаштування

# @where: File name field
# @kind: label
# @max: 12
export-name = Ім'я

# @where: Accessible name of the file name field
# @kind: a11y
# @max: 16
export-name-a11y = Ім'я файлу

# @where: Dialog secondary button
# @kind: button
# @max: 16
export-to-clipboard = У буфер

# @where: Toast after export; $name is the file name
# @kind: toast
# @max: 60
export-done-toast = Експортовано «{ $name }»

# @where: Toast when the file cannot be written
# @kind: error
# @max: 80
export-error = Не вдалося зберегти файл.

# @where: Toast after copying
# @kind: toast
# @max: 24
clipboard-copied = Скопійовано

# @where: Toast when the clipboard is locked or fails
# @kind: error
# @max: 80
clipboard-error = Не вдалося скопіювати в буфер обміну.

## Library
## The home screen: grid of screenshots and videos, filters, details panel.


# @where: Page title
# @kind: title
# @max: 24
lib-title = Бібліотека

# @where: Search field
# @kind: placeholder
# @max: 60
lib-search = Пошук за назвою, тегом або текстом на знімку

# @where: Main button
# @kind: button
# @max: 20
lib-new-shot = Новий знімок

# @where: Arrow next to New screenshot
# @kind: a11y
# @max: 32
lib-capture-more = Інші варіанти захоплення

# @where: Button (v2)
# @kind: button
# @max: 16
lib-record = Записати

# @where: Filter
# @kind: option
# @max: 12
lib-filter-all = Усе

# @where: Filter
# @kind: option
# @max: 14
lib-filter-shots = Знімки

# @where: Filter
# @kind: option
# @max: 12
lib-filter-videos = Відео

# @where: Filter: videos with a DevTools report
# @kind: option
# @max: 16
lib-filter-report = Зі звітом

# @where: Sort menu
# @kind: option
# @max: 20
lib-sort-newest = Нові спочатку

# @where: Sort menu
# @kind: option
# @max: 20
lib-sort-oldest = Старі спочатку

# @where: Sort menu
# @kind: option
# @max: 20
lib-sort-name = За назвою

# @where: Sort menu
# @kind: option
# @max: 20
lib-sort-size = За розміром

# @where: Card badge of a recording with a browser log
# @kind: badge
# @max: 16
lib-badge-devtools = лог DevTools

# @where: Details panel chip
# @kind: button
# @max: 12
lib-add-tag = + тег

# @where: Details panel main button; also card menu (Enter)
# @kind: button
# @max: 24
lib-open-editor = Відкрити в редакторі

# @where: Details panel short button
# @kind: button
# @max: 12
lib-to-agent = Агенту

# @where: Details panel: show the file in Explorer / Finder
# @kind: button
# @max: 16
lib-in-folder = У теці

# @where: Card context menu
# @kind: menu
# @max: 28
lib-show-in-folder = Показати в теці

# @where: Card context menu
# @kind: menu
# @max: 24
lib-trash = У кошик

# @where: Details panel button
# @kind: button
# @max: 24
lib-trash-long = Видалити в кошик

# @where: Details fact
# @kind: label
# @max: 16
lib-source = Джерело

# @where: Source value
# @kind: value
# @max: 16
lib-src-window = Вікно

# @where: Source value
# @kind: value
# @max: 16
lib-src-display = Монітор

# @where: Details fact
# @kind: label
# @max: 12
lib-file = Файл

# @where: Toast after deleting, with an Undo action
# @kind: toast
# @max: 48
lib-trashed-toast = Знімок переміщено в кошик

# @where: Toast action
# @kind: button
# @max: 16
lib-undo = Скасувати

# @where: Toast
# @kind: error
# @max: 60
lib-error-delete = Не вдалося видалити файл.

# @where: Toast
# @kind: error
# @max: 60
lib-error-rename = Не вдалося перейменувати знімок.

# @where: Card badge: this item is open in the editor
# @kind: badge
# @max: 16
lib-in-editor = у редакторі

# @where: Library status bar
# @kind: status
# @max: 60
lib-status =
    { $count ->
        [one] { $count } запис · { $size }
        [few] { $count } записи · { $size }
        [many] { $count } записів · { $size }
       *[other] { $count } запису · { $size }
    }

# @where: Empty library, first run
# @kind: title
# @max: 48
lib-empty-title = Тут з'являтимуться ваші знімки

# @where: Empty library; $key is the region hotkey, e.g. Alt+Shift+4
# @kind: body
# @max: 120
lib-empty-body = Натисніть { $key }, щоб зняти ділянку, або кнопку нижче.

# @where: Empty library, under the button
# @kind: hint
# @max: 60
lib-empty-drop = Перетягніть сюди зображення, щоб відкрити

# @where: Search without results; $query is what the user typed
# @kind: title
# @max: 60
lib-search-empty-title = Нічого не знайдено за «{ $query }»

# @where: Search without results
# @kind: body
# @max: 160
lib-search-empty-body =
    { $count ->
        [one] Шукаємо в назвах, тегах і тексті на знімках. Розпізнавання тексту вимкнене для { $count } старого знімка.
        [few] Шукаємо в назвах, тегах і тексті на знімках. Розпізнавання тексту вимкнене для { $count } старих знімків.
        [many] Шукаємо в назвах, тегах і тексті на знімках. Розпізнавання тексту вимкнене для { $count } старих знімків.
       *[other] Шукаємо в назвах, тегах і тексті на знімках. Розпізнавання тексту вимкнене для { $count } старого знімка.
    }

# @where: Search without results
# @kind: button
# @max: 28
lib-ocr-all = Розпізнати текст на всіх

## Settings
## The Settings page inside the main window: navigation and shared rows.


# @where: Page title
# @kind: title
# @max: 24
set-title = Налаштування

# @where: Search field
# @kind: placeholder
# @max: 28
set-search = Знайти налаштування

# @where: Note next to the title
# @kind: hint
# @max: 32
set-applied-now = Зміни застосовуються одразу

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-shots = Знімки

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-keys = Гарячі клавіші

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-recording = Запис

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-library = Бібліотека

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-sharing = Поширення

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-agents = Агенти й моделі

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-look = Вигляд і мова

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-privacy = Приватність

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-updates = Оновлення

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-about = Про Znimok

## Settings · Hotkeys
## Global hotkeys page.


# @where: Page intro
# @kind: body
# @max: 200
keys-intro = Працюють у будь-якій програмі. Клацніть поле й натисніть комбінацію, щоб змінити. На Mac стандартні ⌘⇧3/4/5 зайняті системою, тому типові інші.

# @where: Master switch
# @kind: label
# @max: 32
keys-enable = Знімати гарячими клавішами

# @where: Under the master switch
# @kind: hint
# @max: 80
keys-enable-hint = Вимкнено — комбінації звільняються для інших програм

# @where: Row
# @kind: label
# @max: 32
keys-region = Знімок ділянки

# @where: Row
# @kind: label
# @max: 32
keys-screen = Знімок усього екрана

# @where: Row
# @kind: label
# @max: 32
keys-window = Знімок вікна

# @where: Row
# @kind: label
# @max: 32
keys-record = Запис відео: старт / стоп

# @where: Row
# @kind: label
# @max: 32
keys-clipboard = Зображення з буфера → редактор

# @where: Row
# @kind: label
# @max: 32
keys-blank = Порожній редактор

# @where: Field while waiting for a combination
# @kind: placeholder
# @max: 28
keys-press = натисніть комбінацію…

# @where: Field without a combination
# @kind: value
# @max: 16
keys-not-set = не задано

# @where: Under a field when the new combination is taken
# @kind: error
# @max: 60
keys-taken = Зайнято іншою програмою — залишено попередню

# @where: Page footer
# @kind: button
# @max: 24
keys-defaults = Повернути типові

# @where: Page footer: canvas navigation
# @kind: hint
# @max: 60
keys-canvas-hint = Зум: Alt + коліщатко · Панорама: пробіл + тягнути

## Settings · Screenshots
## What happens when a capture is released, the pill, editor behaviour.


# @where: Page intro
# @kind: body
# @max: 120
shots-intro = Що відбувається при відпусканні миші в накладці. Кожна дія рівно за одним жестом.

# @where: Section title
# @kind: label
# @max: 48
shots-gestures = Що робить знімок при відпусканні

# @where: Under the section title
# @kind: hint
# @max: 100
shots-gestures-hint = Кожна дія закріплена рівно за одним жестом. Перетягніть, щоб поміняти.

# @where: Gesture chip: release without a key
# @kind: badge
# @max: 16
shots-no-key = без клавіші

# @where: Action
# @kind: option
# @max: 28
shots-act-editor = Відкрити в редакторі

# @where: Action
# @kind: option
# @max: 28
shots-act-clipboard = У буфер обміну

# @where: Action
# @kind: option
# @max: 28
shots-act-overlay = Редагувати поверх екрану

# @where: Row: the pill after a capture
# @kind: label
# @max: 32
shots-pill = Плашка після знімка

# @where: Duration option; $n is a number
# @kind: option
# @max: 8
shots-seconds = { $n } с

# @where: Switch
# @kind: label
# @max: 48
shots-quick-library = Знімки повз редактор теж у бібліотеку

# @where: Switch
# @kind: label
# @max: 60
shots-esc-saves = Esc поверх екрану зберігає, якщо є позначки

# @where: Switch
# @kind: label
# @max: 60
shots-tool-stays = Інструмент лишається активним після малювання

# @where: Switch (same as the title bar toggle)
# @kind: label
# @max: 60
shots-autosave = Зберігати автоматично (тогл є і в заголовку редактора)

# @where: Switch
# @kind: label
# @max: 60
shots-guides = Розумні напрямні: центри, краї, рівні відстані

# @where: Row: what a left click on the tray icon does
# @kind: label
# @max: 48
shots-tray-click = Клік по значку в треї лівою кнопкою

# @where: Option
# @kind: option
# @max: 16
shots-tray-menu = Меню

# @where: Option
# @kind: option
# @max: 20
shots-tray-new = Новий знімок

# @where: Switch
# @kind: label
# @max: 32
shots-cursor = Курсор у знімку

## Settings · Recording
## Video recording settings (v2).


# @where: Page intro
# @kind: body
# @max: 120
rec-intro = Якість і джерела. Звук за замовчуванням вимкнений: ви вмикаєте його свідомо.

# @where: Row
# @kind: label
# @max: 32
rec-fps = Кадрів за секунду

# @where: Row
# @kind: label
# @max: 16
rec-quality = Якість

# @where: Option
# @kind: option
# @max: 12
rec-quality-low = Менша

# @where: Option
# @kind: option
# @max: 12
rec-quality-normal = Звичайна

# @where: Option
# @kind: option
# @max: 12
rec-quality-high = Висока

# @where: Row
# @kind: label
# @max: 12
rec-codec = Кодек

# @where: Row: what clicking a window does in record mode
# @kind: label
# @max: 32
rec-window-click = Клік по вікну

# @where: Option: the frame follows the window
# @kind: option
# @max: 16
rec-follow = слідувати

# @where: Option: record its area only
# @kind: option
# @max: 16
rec-as-region = ділянкою

# @where: Row
# @kind: label
# @max: 24
rec-system-audio = Системний звук

# @where: Device picker default
# @kind: option
# @max: 24
rec-default-device = Типовий пристрій

# @where: Row
# @kind: label
# @max: 24
rec-microphone = Мікрофон

# @where: Row
# @kind: label
# @max: 40
rec-cursor-clicks = Курсор і підсвітка кліків

# @where: Row
# @kind: label
# @max: 40
rec-devtools = Лог DevTools із Chrome/Edge

# @where: Row status
# @kind: status
# @max: 32
rec-extension-connected = розширення під'єднано

## Settings · Library
## Library folder, retention, text recognition.


# @where: Page intro
# @kind: body
# @max: 120
libset-intro = Тека — джерело правди. Її можна покласти на хмарний диск, і Znimok підхопить зміни.

# @where: Row
# @kind: label
# @max: 24
libset-folder = Тека бібліотеки

# @where: Under the folder; $size is formatted
# @kind: hint
# @max: 60
libset-stats =
    { $count ->
        [one] { $count } знімок · { $size }
        [few] { $count } знімки · { $size }
        [many] { $count } знімків · { $size }
       *[other] { $count } знімка · { $size }
    }

# @where: Row
# @kind: label
# @max: 40
libset-keep-shots = Зберігати знімки не більше

# @where: Option
# @kind: option
# @max: 12
libset-by-count = кількість

# @where: Option
# @kind: option
# @max: 12
libset-by-size = обсяг

# @where: Row
# @kind: label
# @max: 32
libset-keep-videos = Відео — не більше

# @where: Switch
# @kind: label
# @max: 48
libset-oldest-trash = Найстаріші — у кошик, а не назавжди

# @where: Switch
# @kind: label
# @max: 60
libset-ocr = Розпізнавати текст на знімках для пошуку (на пристрої)

# @where: Button
# @kind: button
# @max: 20
libset-show-folder = Показати теку

# @where: Button
# @kind: button
# @max: 24
libset-clear = Очистити бібліотеку…

# @where: Confirmation dialog
# @kind: body
# @max: 120
libset-clear-confirm =
    { $count ->
        [one] Перемістити { $count } запис бібліотеки в кошик?
        [few] Перемістити всі { $count } записи бібліотеки в кошик?
        [many] Перемістити всі { $count } записів бібліотеки в кошик?
       *[other] Перемістити всі { $count } запису бібліотеки в кошик?
    }

## Settings · Sharing
## What the Copy button does and the formats.


# @where: Page intro
# @kind: body
# @max: 120
sharing-intro = Що робить головна кнопка «Копіювати» та в якому форматі виходить результат.

# @where: Row
# @kind: label
# @max: 32
sharing-main = Головна кнопка ряду дій

# @where: Option
# @kind: option
# @max: 28
sharing-main-copy = Копіювати в буфер

# @where: Row
# @kind: label
# @max: 24
sharing-format = Формат знімків

# @where: Switch
# @kind: label
# @max: 40
sharing-metadata = Записувати метадані у файли

# @where: Switch
# @kind: label
# @max: 40
sharing-mask = Маскувати секрети у звітах

# @where: Row
# @kind: label
# @max: 16
sharing-file-name = Ім'я файлу

# @where: Under the pattern field; the {date}/{time} tokens in braces are literal and must stay in English
# @kind: hint
# @max: 80
# @note: Shown as "{date}" and "{time}": literal braces (Fluent escapes). Keep the words date and time in English — they are tokens.
sharing-file-pattern-hint = Використовуйте { "{" }date{ "}" } і { "{" }time{ "}" } в імені

# @where: Share targets row
# @kind: hint
# @max: 60
sharing-targets-later = Slack, Jira, Telegram, Redmine — згодом

## Settings · Privacy
## Everything that touches the network and data.


# @where: Page intro
# @kind: body
# @max: 120
priv-intro = Znimok нікуди нічого не надсилає без вашої дії. Тут — усе, що торкається мережі й даних.

# @where: Switch
# @kind: label
# @max: 40
priv-updates = Перевіряти оновлення щодня

# @where: Under the switch
# @kind: hint
# @max: 48
priv-updates-hint = GitHub Releases, лише номер версії

# @where: Row
# @kind: label
# @max: 32
priv-telemetry = Телеметрія й статистика

# @where: Row value
# @kind: value
# @max: 40
priv-telemetry-none = немає, і перемикача теж

# @where: Row
# @kind: label
# @max: 24
priv-crash = Звіти про збої

# @where: Row value
# @kind: value
# @max: 40
priv-crash-local = локальний файл; надсилаєте самі

# @where: Row
# @kind: label
# @max: 40
priv-cloud = Хмарні моделі (власний ключ)

# @where: Row value
# @kind: value
# @max: 32
priv-cloud-off = вимкнено, ключа немає

# @where: Row action
# @kind: button
# @max: 20
priv-add-key = Додати ключ…

# @where: Row
# @kind: label
# @max: 40
priv-agents = Доступ для агентів (MCP)

# @where: Row value
# @kind: value
# @max: 40
priv-agents-count =
    { $count ->
        [one] { $count } клієнт має дозвіл
        [few] { $count } клієнти мають дозвіл
        [many] { $count } клієнтів мають дозвіл
       *[other] { $count } клієнта мають дозвіл
    }

# @where: Row (macOS)
# @kind: label
# @max: 32
priv-mac-perms = Дозволи macOS

# @where: Row action
# @kind: button
# @max: 16
priv-check = Перевірити

# @where: Link
# @kind: button
# @max: 28
priv-policy = Політика приватності

# @where: Link
# @kind: button
# @max: 24
priv-source = Вихідний код

## Settings · Appearance and About
## Theme, language, launch at login, About.


# @where: Row
# @kind: label
# @max: 12
look-theme = Тема

# @where: Option
# @kind: option
# @max: 20
look-theme-system = Як у системі

# @where: Option
# @kind: option
# @max: 12
look-theme-light = Світла

# @where: Option
# @kind: option
# @max: 12
look-theme-dark = Темна

# @where: Row
# @kind: label
# @max: 12
look-language = Мова

# @where: Option: follow the system
# @kind: option
# @max: 16
look-language-system = Системна

# @where: Name of THIS language in itself, for the language menu
# @kind: option
# @max: 20
# @note: Write the language's own name in the language itself (endonym).
look-language-name = Українська

# @where: Row
# @kind: label
# @max: 32
look-tool-labels = Підписи під інструментами

# @where: Option
# @kind: option
# @max: 12
look-labels-always = завжди

# @where: Option
# @kind: option
# @max: 16
look-labels-first = перші запуски

# @where: Option
# @kind: option
# @max: 12
look-labels-never = ніколи

# @where: Switch
# @kind: label
# @max: 40
look-autostart = Запускати при вході в систему

# @where: Row
# @kind: label
# @max: 24
look-reduce-motion = Зменшити рух

# @where: Row value
# @kind: value
# @max: 20
look-as-system = як у системі

# @where: About: version line
# @kind: label
# @max: 40
about-version = Znimok { $version }

# @where: About: after the version
# @kind: badge
# @max: 40
about-signed = підпис релізу перевірено

# @where: About: credits; Slint and font names stay as they are
# @kind: body
# @max: 200
about-made-with = Зроблено з Slint · шрифти Onest, JetBrains Mono, Unbounded (OFL) · ліцензії бібліотек

# @where: About: donation link
# @kind: button
# @max: 16
about-support = Підтримати

# @where: About: install an update
# @kind: button
# @max: 16
about-update = Оновити

# @where: Banner in the library and About
# @kind: title
# @max: 40
update-available = Доступна версія { $version }

# @where: Banner second line; $size formatted
# @kind: body
# @max: 100
update-details = Підписано, { $size }. Встановиться під час наступного запуску.

# @where: Banner action
# @kind: button
# @max: 20
update-now = Оновити зараз

## Onboarding
## First run: permissions (macOS), hotkeys, library folder, agents.


# @where: macOS first run, big title
# @kind: title
# @max: 48
onb-title-mac = Три дозволи — і Znimok готовий

# @where: macOS first run, under the title
# @kind: body
# @max: 200
onb-intro-mac = macOS питає дозволу на все, що бачить екран. Znimok працює лише на вашому Mac і нікуди нічого не надсилає без вашої дії.

# @where: Windows first run, big title
# @kind: title
# @max: 48
onb-title-win = Два кроки — і Znimok готовий

# @where: Windows first run, under the title
# @kind: body
# @max: 200
onb-intro-win = Znimok не потребує прав адміністратора й нікуди нічого не надсилає без вашої дії. Перевірте гарячі клавіші й виберіть, де житимуть знімки.

# @where: Under the progress dots
# @kind: hint
# @max: 80
onb-step = Крок { $n } з { $total } · можна повернутися сюди пізніше в Налаштуваннях

# @where: Step (macOS): the Screen Recording permission
# @kind: label
# @max: 40
onb-screen = Запис екрана й системного звуку

# @where: Step state
# @kind: hint
# @max: 60
onb-screen-granted = Надано. Потрібно для будь-якого знімка.

# @where: Step state before the grant
# @kind: hint
# @max: 80
onb-screen-needed = Потрібно для будь-якого знімка. macOS попросить перезапустити Znimok.

# @where: Step
# @kind: label
# @max: 24
onb-keys = Гарячі клавіші

# @where: Step text (macOS); $region, $screen, $record are key combos like ⌃⇧4
# @kind: body
# @max: 300
onb-keys-mac = Дозволу не потрібно. Типові: { $region } ділянка, { $screen } екран, { $record } запис. Звичні ⌘⇧3/4/5 зайняті системою: вимкніть їх у Системних параметрах → Клавіатура → Клавіатурні скорочення, і Znimok перейме їх сам.

# @where: Step text (Windows)
# @kind: body
# @max: 240
onb-keys-win = Типові: { $region } ділянка, { $screen } екран, { $record } запис. Якщо комбінацію зайняла інша програма, оберіть іншу тут.

# @where: Step action (macOS)
# @kind: button
# @max: 24
onb-open-settings = Відкрити налаштування

# @where: Step
# @kind: label
# @max: 24
onb-library = Тека бібліотеки

# @where: Step text
# @kind: hint
# @max: 100
onb-library-hint = Усі знімки й відео лежать тут як файли. Можна вибрати теку на хмарному диску.

# @where: Step (optional)
# @kind: label
# @max: 24
onb-mic = Мікрофон

# @where: Step text
# @kind: hint
# @max: 80
onb-mic-hint = Лише для запису відео з голосом. Можна пропустити.

# @where: Step action
# @kind: button
# @max: 16
onb-allow = Дозволити

# @where: Step
# @kind: label
# @max: 32
onb-agents = Доступ для AI-агентів

# @where: Step text
# @kind: hint
# @max: 100
onb-agents-hint = Вимкнено. Увімкнути можна на сторінці «Агенти», коли знадобиться.

# @where: Footer
# @kind: button
# @max: 20
onb-skip-all = Пропустити все

## States and errors
## Empty states, errors, confirmations, toasts. Explain without blaming the user; one action each.


# @where: Error title
# @kind: title
# @max: 48
err-capture-title = Знімок не вдалося зробити

# @where: Error body (macOS, no permission)
# @kind: body
# @max: 200
err-capture-mac-perm = macOS не дозволяє Znimok бачити екран. Увімкніть «Запис екрана й системного звуку» в Системних параметрах і поверніться.

# @where: Error action
# @kind: button
# @max: 32
err-open-system-settings = Відкрити Системні параметри

# @where: Error body (other causes); $reason is a short technical reason
# @kind: body
# @max: 160
err-capture-generic = Система відмовила в захопленні: { $reason }

# @where: Error title; $combo like Alt+Shift+4
# @kind: title
# @max: 60
err-key-taken-title = { $combo } вже використовує інша програма

# @where: Error body; $action is e.g. "Region screenshot"
# @kind: body
# @max: 160
err-key-taken-body = { $action } поки без гарячої клавіші. Оберіть іншу комбінацію або звільніть цю в тій програмі.

# @where: Error action
# @kind: button
# @max: 20
err-key-choose = Обрати іншу

# @where: Error title
# @kind: title
# @max: 48
err-disk-title = Запис зупинено: диск заповнений

# @where: Error body; $time like 0:41
# @kind: body
# @max: 160
err-disk-body = Записане до цього моменту збережено ({ $time }). Звільніть місце або змініть теку бібліотеки.

# @where: Error action
# @kind: button
# @max: 20
err-open-recording = Відкрити запис

# @where: Error action
# @kind: button
# @max: 20
err-change-folder = Змінити теку

# @where: Toast when saving to the library fails
# @kind: error
# @max: 100
err-library-save = Не вдалося зберегти в бібліотеку. Перевірте, чи доступна тека бібліотеки.

# @where: Hint when the retention limit is reached
# @kind: error
# @max: 100
err-library-full = Ліміт досягнуто: наступне збереження перемістить найстаріший запис у кошик.

# @where: Close-with-unsaved dialog; $name is the document name
# @kind: title
# @max: 60
confirm-save-title = Зберегти зміни в «{ $name }»?

# @where: Close-with-unsaved dialog
# @kind: body
# @max: 120
confirm-save-body = Незбережене буде втрачено.

# @where: Toast with progress
# @kind: toast
# @max: 32
toast-exporting-video = Експорт відео…

# @where: Toast action
# @kind: button
# @max: 16
toast-stop = Зупинити

## Agents
## The Agents page: connected MCP clients, models, action log, and the permission request.


# @where: Page title
# @kind: title
# @max: 24
agents-title = Агенти

# @where: Yellow live indicator; $client is the agent name
# @kind: badge
# @max: 48
agents-live = { $client } зараз знімає екран

# @where: Page action
# @kind: button
# @max: 32
agents-revoke-all = Відкликати всі дозволи

# @where: Section
# @kind: heading
# @max: 32
agents-clients = Під'єднані клієнти

# @where: Section hint
# @kind: hint
# @max: 120
agents-clients-hint = Кожен агент отримує свій ключ і свої дозволи. Доступ вимкнено, поки ви його не ввімкнете.

# @where: Client status
# @kind: status
# @max: 16
agents-active = Активний

# @where: Client status; $when is relative, e.g. "yesterday"
# @kind: status
# @max: 32
agents-last-used = Востаннє { $when }

# @where: Client row "…" button
# @kind: a11y
# @max: 24
agents-configure = Налаштувати

# @where: Permission name in summaries
# @kind: label
# @max: 24
agents-scope-screen = знімок екрана

# @where: Permission name in summaries
# @kind: label
# @max: 24
agents-scope-library = читання бібліотеки

# @where: Permission name in summaries
# @kind: label
# @max: 24
agents-scope-marks = позначки

# @where: Permission name in summaries
# @kind: label
# @max: 24
agents-scope-export = експорт

# @where: Summary: permission level
# @kind: label
# @max: 24
agents-grant-always = завжди

# @where: Summary: permission level
# @kind: label
# @max: 24
agents-grant-ask = питати щоразу

# @where: Summary: permission level
# @kind: label
# @max: 24
agents-grant-no = ні

# @where: Dashed button under the list
# @kind: button
# @max: 40
agents-how-connect = Як під'єднати інший агент

# @where: Section
# @kind: heading
# @max: 40
agents-models = Моделі для функцій Znimok

# @where: Section hint
# @kind: hint
# @max: 140
agents-models-hint = Спершу на пристрої. Хмара — лише з вашим ключем і лише для функцій, які ви дозволили.

# @where: Model row
# @kind: label
# @max: 48
agents-ocr = Розпізнавання тексту на знімку

# @where: Model row
# @kind: label
# @max: 48
agents-redact = Пошук секретів і облич для маскування

# @where: Model row value
# @kind: status
# @max: 20
agents-on-device = На пристрої

# @where: Model row
# @kind: label
# @max: 40
agents-assistant = Помічник природною мовою

# @where: Model row second line; $model is a model name
# @kind: hint
# @max: 60
agents-assistant-hint = { $model } · ваш ключ · показувати, що відправляється

# @where: Row: money spent via the user's key
# @kind: label
# @max: 32
agents-spend = Витрати за місяць

# @where: Section
# @kind: heading
# @max: 24
agents-log = Журнал дій

# @where: Section action
# @kind: button
# @max: 12
agents-log-export = Експорт

# @where: Log entry
# @kind: label
# @max: 80
agents-log-window-shot = { $client } · знімок вікна «{ $title }»

# @where: Log entry
# @kind: label
# @max: 60
agents-log-window-list = { $client } · список вікон

# @where: Log entry; $format like PNG
# @kind: label
# @max: 80
agents-log-export-entry = { $client } · експорт { $format } «{ $name }»

# @where: Log entry
# @kind: label
# @max: 60
agents-log-marks =
    { $count ->
        [one] { $client } · додано { $count } позначку
        [few] { $client } · додано { $count } позначки
        [many] { $client } · додано { $count } позначок
       *[other] { $client } · додано { $count } позначки
    }

# @where: Log entry
# @kind: label
# @max: 60
agents-log-denied = { $client } · знімок екрана — відхилено

# @where: Empty state
# @kind: title
# @max: 48
agents-empty-title = Жоден агент ще не звертався

# @where: Empty state
# @kind: body
# @max: 160
agents-empty-body = Znimok може працювати з Claude Code та іншими агентами. Доступ вимкнений, поки ви його не ввімкнете.

# @where: Empty state main action
# @kind: button
# @max: 24
agents-enable = Увімкнути доступ

# @where: Empty state second action
# @kind: button
# @max: 20
agents-how = Як під'єднати

# @where: Accessible name of the permission dialog
# @kind: a11y
# @max: 40
perm-a11y = Запит дозволу від агента

# @where: Permission dialog title
# @kind: title
# @max: 60
perm-title = { $client } хоче знімати екран

# @where: Under the title; $key is a shortened key
# @kind: hint
# @max: 48
perm-new-client = Новий клієнт · ключ { $key }

# @where: Permission dialog text
# @kind: body
# @max: 260
perm-body = Агент зможе робити знімки екрана й вікон, поки ви не відкличете дозвіл. Під час знімка ви бачитимете жовтий індикатор, а кожна дія потрапляє в журнал.

# @where: Checkbox
# @kind: label
# @max: 40
perm-scope-screen = Знімки екрана й вікон

# @where: Checkbox
# @kind: label
# @max: 40
perm-scope-marks = Позначки й експорт

# @where: Checkbox
# @kind: label
# @max: 40
perm-scope-library = Читання бібліотеки

# @where: Grant for this one request
# @kind: button
# @max: 16
perm-once = Лише зараз

# @where: Grant until the agent disconnects
# @kind: button
# @max: 16
perm-session = На цю сесію

# @where: Grant permanently
# @kind: button
# @max: 16
perm-always = Завжди

# @where: Refuse
# @kind: button
# @max: 16
perm-deny = Відхилити

## Assistant
## Ctrl+K command bar: natural language → a plan of commands. Dialogs from the owner's decisions of 28.09.2026.


# @where: Title bar button and dialog name
# @kind: button
# @max: 16
asst-name = Помічник

# @where: Accessible name of the command field
# @kind: a11y
# @max: 16
asst-command = Команда

# @where: Empty command field
# @kind: placeholder
# @max: 60
asst-placeholder = Опишіть, що зробити зі знімком…

# @where: Plan header; $model is a model name, $sent says what went to the cloud
# @kind: status
# @max: 120
asst-plan =
    { $count ->
        [one] План з { $count } кроку · { $model } · відправлено: { $sent }
        [few] План з { $count } кроків · { $model } · відправлено: { $sent }
        [many] План з { $count } кроків · { $model } · відправлено: { $sent }
       *[other] План з { $count } кроку · { $model } · відправлено: { $sent }
    }

# @where: Value of $sent: no pixels were sent
# @kind: label
# @max: 80
asst-sent-structure = список вікон і розмір екрана, без знімка

# @where: Plan step state
# @kind: badge
# @max: 24
asst-preview = попередній перегляд

# @where: Under the plan
# @kind: hint
# @max: 100
asst-dashed-note = Пунктиром — що буде додано. Можна відредагувати запит і повторити.

# @where: Suggestions heading
# @kind: heading
# @max: 20
asst-try = Спробуйте також

# @where: Example request chip (written in the user's voice)
# @kind: option
# @max: 48
asst-example-1 = розмий усі e-mail і телефони

# @where: Example request chip
# @kind: option
# @max: 48
asst-example-2 = пронумеруй кнопки зліва направо

# @where: Example request chip
# @kind: option
# @max: 48
asst-example-3 = обріж до вікна налаштувань

# @where: Example request chip
# @kind: option
# @max: 48
asst-example-4 = знайди вчорашній знімок терміналу

# @where: Indicator: the request goes to the cloud
# @kind: badge
# @max: 16
asst-cloud = у хмару

# @where: Indicator: no network
# @kind: badge
# @max: 16
asst-offline = немає зв'язку

# @where: Local palette state without an API key
# @kind: hint
# @max: 100
asst-no-key = Хмарні підказки потребують вашого ключа API. Локальні команди працюють і без нього.

# @where: Action in the no-key state
# @kind: button
# @max: 24
asst-set-up-key = Додати ключ…

# @where: Dialog before pixels leave the device
# @kind: title
# @max: 40
asst-send-title = Надіслати знімок?

# @where: Dialog text
# @kind: body
# @max: 200
asst-send-body = Для цієї команди потрібне зображення. Його спершу замасковано на пристрої; надіслано буде саме те, що ви бачите.

# @where: Checkbox
# @kind: label
# @max: 48
asst-send-dont-ask-doc = Не питати для цього документа

# @where: Checkbox (global; reset in Settings)
# @kind: label
# @max: 48
asst-send-dont-ask = Більше не питати

# @where: Dialog main action
# @kind: button
# @max: 16
asst-send = Надіслати

# @where: Dialog second action
# @kind: button
# @max: 20
asst-dont-send = Не надсилати

# @where: Dialog on the first cloud action
# @kind: title
# @max: 60
asst-cost-title = Хмарні функції платні

# @where: Dialog text; $model name, $price is a formatted amount like $0.01
# @kind: body
# @max: 300
asst-cost-body = Запити йдуть до { $model } з вашим ключем, і Anthropic виставляє рахунок на ваш акаунт. Типовий запит зі знімком 1024 px коштує приблизно { $price }. Витрати видно на сторінці «Агенти».

# @where: Dialog main action
# @kind: button
# @max: 20
asst-cost-ok = Зрозуміло

# @where: Note under the bar
# @kind: hint
# @max: 160
asst-footnote = Помічник — той самий шар команд, що й MCP: кожен крок плану = команда, яку можна скасувати одним Ctrl+Z.

## Recording and video (v2)
## Screen recording, the video editor and video export. Shipped in v2; translated now so the layout is stable.


# @where: Recording pill button
# @kind: a11y
# @max: 16
rec-pause = Пауза

# @where: Recording pill button while paused
# @kind: a11y
# @max: 16
rec-resume = Продовжити

# @where: Recording pill button
# @kind: a11y
# @max: 16
rec-stop = Стоп

# @where: Tray / menu bar text while recording; $time like 00:12
# @kind: status
# @max: 24
rec-tray = Запис { $time }

# @where: Status while paused
# @kind: status
# @max: 24
rec-paused = На паузі { $time }

# @where: Pill after recording
# @kind: title
# @max: 32
rec-saved = Відео збережено

# @where: Pill second line
# @kind: hint
# @max: 60
rec-saved-details = { $width } × { $height } · { $size }

# @where: DevTools log size
# @kind: badge
# @max: 32
rec-devtools-events =
    { $count ->
        [one] лог DevTools: { $count } подія
        [few] лог DevTools: { $count } події
        [many] лог DevTools: { $count } подій
       *[other] лог DevTools: { $count } події
    }

# @where: Pill button
# @kind: button
# @max: 16
rec-share = Поділитися

# @where: Video editor title bar
# @kind: button
# @max: 20
vid-bug-report = Звіт про баг

# @where: Status hint; "Space" is the key
# @kind: hint
# @max: 24
vid-hint-play = Пробіл — пуск

# @where: Status hint
# @kind: hint
# @max: 24
vid-hint-frame = ← → — кадр

# @where: Status hint
# @kind: hint
# @max: 24
vid-hint-cut = S — розріз

# @where: Transport
# @kind: a11y
# @max: 16
vid-to-start = На початок

# @where: Transport
# @kind: a11y
# @max: 16
vid-frame-back = Кадр назад

# @where: Transport
# @kind: a11y
# @max: 16
vid-play = Пуск

# @where: Transport
# @kind: a11y
# @max: 16
vid-pause = Пауза

# @where: Transport
# @kind: a11y
# @max: 16
vid-frame-forward = Кадр уперед

# @where: Transport
# @kind: a11y
# @max: 16
vid-to-end = У кінець

# @where: Transport
# @kind: a11y
# @max: 16
vid-speed = Швидкість

# @where: Transport
# @kind: a11y
# @max: 16
vid-loop = Повтор

# @where: Track header
# @kind: label
# @max: 12
vid-track-system = Система

# @where: Track header
# @kind: label
# @max: 12
vid-track-mic = Мікрофон

# @where: Button: take the current frame as a screenshot
# @kind: button
# @max: 24
vid-frame-as-shot = Кадр як знімок

# @where: Timeline lane label (short)
# @kind: label
# @max: 8
vid-lane-video = відео

# @where: Timeline lane label (short)
# @kind: label
# @max: 8
vid-lane-sound = звук

# @where: Timeline lane label (short)
# @kind: label
# @max: 8
vid-lane-mic = мікр.

# @where: Timeline lane label (short)
# @kind: label
# @max: 8
vid-lane-marks = позн.

# @where: Clip tab section
# @kind: label
# @max: 16
vid-trim = Обрізання

# @where: Trim field
# @kind: label
# @max: 8
vid-from = Від

# @where: Trim field
# @kind: label
# @max: 8
vid-to = До

# @where: Under the trim; $seconds formatted like 4.9
# @kind: hint
# @max: 100
vid-cuts =
    { $count ->
        [one] Вирізано { $count } проміжок · { $seconds } с. Delete — вирізати виділене.
        [few] Вирізано { $count } проміжки · { $seconds } с. Delete — вирізати виділене.
        [many] Вирізано { $count } проміжків · { $seconds } с. Delete — вирізати виділене.
       *[other] Вирізано { $count } проміжку · { $seconds } с. Delete — вирізати виділене.
    }

# @where: Clip tab section
# @kind: label
# @max: 12
vid-sound = Звук

# @where: Under the sound section
# @kind: hint
# @max: 80
vid-tracks-note = Доріжки окремі; при експорті зводяться в одну.

# @where: Clip tab section
# @kind: label
# @max: 24
vid-size-frame = Розмір і кадр

# @where: Events tab section
# @kind: label
# @max: 16
vid-devtools = Лог DevTools

# @where: Events count
# @kind: status
# @max: 16
vid-events =
    { $count ->
        [one] { $count } подія
        [few] { $count } події
        [many] { $count } подій
       *[other] { $count } події
    }

# @where: Status bar: current frame number
# @kind: status
# @max: 16
vid-frame-n = кадр { $n }

# @where: Status bar
# @kind: status
# @max: 40
vid-marks-here =
    { $count ->
        [one] { $count } позначка на цьому кадрі
        [few] { $count } позначки на цьому кадрі
        [many] { $count } позначок на цьому кадрі
       *[other] { $count } позначки на цьому кадрі
    }

# @where: Status bar
# @kind: status
# @max: 32
vid-cursor-recorded = Курсор і кліки записано

# @where: Status bar; $fps is a number
# @kind: status
# @max: 16
vid-fps = { $fps } к/с

# @where: Video export dialog
# @kind: title
# @max: 24
vexp-title = Експорт відео

# @where: Field
# @kind: label
# @max: 16
vexp-fps = Кадрів/с

# @where: Field
# @kind: label
# @max: 12
vexp-width = Ширина

# @where: Field (GIF)
# @kind: label
# @max: 12
vexp-colours = Кольорів

# @where: Checkbox (GIF)
# @kind: label
# @max: 16
vexp-dither = Дизеринг

# @where: Checkbox (GIF)
# @kind: label
# @max: 16
vexp-loop = Повторювати

# @where: Before the estimate
# @kind: label
# @max: 12
vexp-estimate = Оцінка:

# @where: Warning for large GIFs
# @kind: hint
# @max: 140
vexp-gif-warning = GIF понад 25 МБ погано вантажаться в чатах. Для таких кліпів кращий MP4 або WebP.
