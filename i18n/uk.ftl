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

# @where: Option: size taken from the content (text block width)
# @kind: option
# @max: 8
common-auto = Авто

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

# @where: Question: the document clicked in the library is open in another window
# @kind: heading
# @max: 80
open-twice-title = «{ $name }» уже відкрито в іншому вікні

# @where: Question body: why a copy, not a second window on the same file
# @kind: body
# @max: 200
open-twice-body = Два вікна, що зберігають один документ, затиратимуть зміни одне одного. Перейдіть до того вікна або відкрийте копію — вона збережеться як новий документ.

# @where: Button: bring the window with the document to the front
# @kind: button
# @max: 24
open-twice-go = Перейти до вікна

# @where: Button: open a copy that saves as a new document
# @kind: button
# @max: 24
open-twice-copy = Відкрити копію

# @where: The name of a copy of a document
# @kind: label
# @max: 80
doc-copy-name = { $name } (копія)

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

# @where: Tooltip tail of the Copy button: it can be dragged out as a file
# @kind: tooltip
# @max: 32
share-drag-tip = перетягніть у чат чи теку

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

# @where: Other ways menu: a copy of the document (.znimok) in any folder
# @kind: menu
# @max: 24
doc-save-as = Зберегти як…

# @where: Status line after «Save as…»
# @kind: toast
# @max: 48
doc-saved-as = Збережено «{ $name }»

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

# @where: Tool rail button tooltip and accessible name; (L) is its shortcut. Heads are properties of the line
# @kind: tooltip
# @max: 28
tool-line = Лінія (L)

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

# @where: Inspector title of a selected counter: its numbering group
# @kind: heading
# @max: 32
counter-group-title = Лічильник — Група { $n }

# @where: Inspector, Counter: button that selects the whole numbering group (tooltip says more)
# @kind: button
# @max: 24
counter-edit-group-short = Редагувати всю групу

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

# @where: Tooltip of the Image button, after its name
# @kind: tooltip
# @max: 96
tool-image-tip = Покласти на знімок картинку з файлу (Ctrl+V вставляє з буфера)

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

# @where: Tooltip of the "none" swatch in a text's outline colours
# @kind: tooltip
# @max: 32
ctx-outline-none = Без обводки

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

# @where: Toast while the eyedropper is armed: the next click on the picture picks its colour
# @kind: hint
# @max: 80
colour-eyedropper-hint = Клацніть на знімку, щоб узяти колір · Esc — скасувати

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

# @where: Object panel: row label of the line thickness control
# @kind: label
# @max: 12
insp-thickness = Товщина

# @where: Object panel: the "no outline" chip in the stroke colour row (rectangle, ellipse)
# @kind: tooltip
# @max: 24
insp-stroke-none = Без контуру

# @where: Object panel: section with X / Y / W / H of the selected annotation
# @kind: label
# @max: 24
insp-position = Положення й розмір

# @where: Object panel: section with X / Y of an annotation whose size is not typed (text, pen, marker, counter, stamp)
# @kind: label
# @max: 24
insp-place = Положення

# @where: Inspector, Marker: tooltip of the fourth (thickest) thickness button
# @kind: tooltip
# @max: 24
marker-extra-thick = Дуже товста

# @where: Fill section: button that swaps the stroke and fill colours (text: letters and outline)
# @kind: tooltip
# @max: 40
insp-swap-colours = Поміняти контур і заливку

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

# @where: Effect strength option: weak
# @kind: option
# @max: 12
insp-effect-light = Легка

# @where: Effect strength option: strong
# @kind: option
# @max: 12
insp-effect-strong = Сильна

# @where: Position field label; keep the Latin letter
# @kind: label
# @max: 2
insp-x = X

# @where: Position field label; keep the Latin letter
# @kind: label
# @max: 2
insp-y = Y

# @where: Line start point, X; keep the Latin letter
# @kind: label
# @max: 3
insp-x1 = X1

# @where: Line start point, Y
# @kind: label
# @max: 3
insp-y1 = Y1

# @where: Line end point, X
# @kind: label
# @max: 3
insp-x2 = X2

# @where: Line end point, Y
# @kind: label
# @max: 3
insp-y2 = Y2

# @where: Section of a line: its two ends
# @kind: label
# @max: 24
insp-line-points = Кінці лінії

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

# @where: Inspector, Counter: caption of the counter's own colour row
# @kind: label
# @max: 20
counter-colour = Колір

# @where: Inspector, Counter: the button between the colour and the number rows
# @kind: tooltip
# @max: 48
counter-swap = Поміняти колір і цифру

# @where: Inspector, Counter: the row of fixed sizes S M L XL
# @kind: label
# @max: 12
counter-size = Розмір

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

# @where: Stamp picker: heading of the emoji grid
# @kind: label
# @max: 16
stamp-emoji = Емодзі

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

# @where: Layers panel, under the list: how to reorder and group by dragging
# @kind: hint
# @max: 90
layers-drag-hint = Тягніть рядок, щоб змінити порядок; покладіть на інший рядок — щоб згрупувати.

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

# @where: Object panel: section with order, align, spread and group controls
# @kind: label
# @max: 16
arrange-title = Розташування

# @where: Object panel: row label of the z-order buttons
# @kind: label
# @max: 12
arrange-order = Порядок

# @where: Object panel: row label of the six align buttons
# @kind: label
# @max: 12
arrange-align = Вирівняти

# @where: Object panel: row label of the even-gaps buttons
# @kind: label
# @max: 12
arrange-distribute = Розподіл

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

# @where: Hint chip: the mouse wheel / two-finger scroll
# @kind: badge
# @max: 12
capture-wheel = коліщатко

# @where: Hint after the wheel chip: it turns the magnifier on and zooms it
# @kind: hint
# @max: 20
capture-magnifier = лупа

# @where: Hint chip: a second click (a double click, or a click and then a drag)
# @kind: badge
# @max: 16
capture-double = подвійний клік

# @where: Hint after that chip: the shot is taken after a 3-2-1 countdown
# @kind: hint
# @max: 20
capture-delayed = через 3 с

# @where: Hint after the "Q" chip: read QR codes and barcodes in the selection or the screen
# @kind: hint
# @max: 20
capture-qr = QR-код

# @where: Hint after the "S" chip: a scrolling capture of the highlighted window or region
# @kind: hint
# @max: 20
capture-scroll = з прокруткою

# @where: Hint after "Esc"
# @kind: hint
# @max: 20
capture-cancel = скасувати

# @where: Capture overlay of a recording, hint strip: what releasing does
# @kind: hint
# @max: 20
capture-record = записати

# @where: Capture overlay of a recording, hint strip beside the A key: $mode is the sound choice in lower case («system sound»)
# @kind: hint
# @max: 40
capture-sound = звук: { $mode }

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

# @where: Scrolling capture panel: the height stitched so far; $height is in pixels
# @kind: status
# @max: 40
scroll-progress = Прокрутка… { $height } px

# @where: Scrolling capture panel, when automatic scrolling does not move the content
# @kind: hint
# @max: 48
scroll-manual = Прокручуйте самі — Znimok встигає

# @where: Scrolling capture panel, under the height while Znimok scrolls
# @kind: hint
# @max: 48
scroll-auto = Esc — скасувати

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

# @where: Tray / menu bar item: read QR codes and barcodes on the screen under the pointer
# @kind: menu
# @max: 40
tray-read-codes = Зчитати QR-код з екрана

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

# @where: Item of the Copy button's menu; toggles writing title, description, author and date into files
# @kind: menu
# @max: 28
export-metadata-menu = Записати метадані

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

# @where: Export sheet: the format card titles
# @kind: label
# @max: 12
export-png = PNG

# @where: Export sheet: the format card titles
# @kind: label
# @max: 12
export-jpeg = JPEG

# @where: Export sheet: the format card titles
# @kind: label
# @max: 12
export-webp = WebP

# @where: Export sheet: PNG card, beside the title
# @kind: label
# @max: 20
export-png-sub = без втрат

# @where: Export sheet: JPEG card, beside the title
# @kind: label
# @max: 20
export-jpeg-sub = з якістю

# @where: Export sheet: WebP card, beside the title
# @kind: label
# @max: 20
export-webp-sub = найменші файли

# @where: Export sheet: PNG card, what it is for
# @kind: hint
# @max: 90
export-png-desc = Кожен піксель як є, з прозорістю — для інтерфейсів і тексту.

# @where: Export sheet: JPEG card, what it is for
# @kind: hint
# @max: 90
export-jpeg-desc = Найменший для фото й градієнтів; без прозорості.

# @where: Export sheet: WebP card, what it is for
# @kind: hint
# @max: 90
export-webp-desc = Найменші файли — з якістю або без втрат; відкривається в браузерах і чатах.

# @where: Export sheet, WebP chosen: a switch; off = the quality slider (libwebp)
# @kind: label
# @max: 30
export-lossless = Без втрат

# @where: Export sheet: under the title — the picture's size and the size it will have
# @kind: status
# @max: 80
export-subtitle = { $w } × { $h } → { $tw } × { $th } px · позначки вмальовано

# @where: Export sheet: caption of the row «To clipboard / File… / To the library»
# @kind: label
# @max: 16
export-where = Куди

# @where: Export sheet: where to — a file (the save dialog comes next)
# @kind: button
# @max: 16
export-to-file = Файл…

# @where: Export sheet: where to — a flat copy in the library
# @kind: button
# @max: 20
export-to-library = У бібліотеку

# @where: Export sheet: note when the clipboard is chosen
# @kind: hint
# @max: 100
export-clipboard-note = У буфер іде саме зображення; формат і якість — для файлів.

# @where: Export sheet: the main button for a file
# @kind: button
# @max: 24
export-go = Експортувати { $format }

# @where: Export sheet: the main button for the clipboard
# @kind: button
# @max: 24
export-go-copy = Скопіювати

# @where: Export sheet: the main button for the library
# @kind: button
# @max: 24
export-go-library = Зберегти в бібліотеку

# @where: Toast: the flat copy went to the library
# @kind: toast
# @max: 48
export-library-done = Пласка копія — у бібліотеці

# @where: A file size in kilobytes
# @kind: label
# @max: 12
size-kb = { $n } КБ

# @where: A file size in megabytes
# @kind: label
# @max: 12
size-mb = { $n } МБ

# @where: Toast after copying
# @kind: toast
# @max: 24
clipboard-copied = Скопійовано

# @where: Title of the list of codes found; $count is how many
# @kind: title
# @max: 40
codes-title = Знайдено кодів: { $count }

# @where: Title when no code was found
# @kind: title
# @max: 40
codes-none-title = Кодів немає

# @where: Body when no code was found
# @kind: body
# @max: 120
codes-none-body = На цій картинці немає QR-кодів чи штрихкодів.

# @where: A found code: a web link
# @kind: body
# @max: 200
codes-link = Посилання: { $url }

# @where: A found code: a Wi-Fi network with a password
# @kind: body
# @max: 200
codes-wifi = Wi-Fi «{ $ssid }» · пароль: { $password } · { $security }

# @where: A found code: a Wi-Fi network without a password
# @kind: body
# @max: 120
codes-wifi-open = Wi-Fi «{ $ssid }» без пароля

# @where: A found code: a contact card
# @kind: label
# @max: 32
codes-contact = Контакт

# @where: A found code: a calendar event
# @kind: label
# @max: 32
codes-event = Подія

# @where: A found code: an e-mail address
# @kind: body
# @max: 120
codes-email = Пошта: { $address }

# @where: A found code: a phone number
# @kind: body
# @max: 80
codes-phone = Телефон: { $number }

# @where: A found code: plain text
# @kind: body
# @max: 200
codes-text = Текст: { $text }

# @where: Button: copy the text of the codes
# @kind: button
# @max: 24
codes-copy = Копіювати текст

# @where: Button: open the link of a code (a confirmation follows)
# @kind: button
# @max: 24
codes-open-link = Відкрити посилання…

# @where: Question before a link from a code opens in the browser
# @kind: title
# @max: 40
codes-open-title = Відкрити посилання?

# @where: Body of that question; $url is the whole address
# @kind: body
# @max: 400
codes-open-body = { $url } — перевірте всю адресу: через QR-коди часто підсовують фішингові сторінки.

# @where: Button: open the link
# @kind: button
# @max: 16
codes-open = Відкрити

# @where: Editor, Image tab: button to read QR codes and barcodes on the picture
# @kind: button
# @max: 28
img-read-codes = Зчитати QR-коди й штрихкоди

# @where: Image tab: button that reads the text on the screenshot (on the device, no AI)
# @kind: button
# @max: 32
img-read-text = Текст зі знімка

# @where: Title of the panel with the text found on the screenshot
# @kind: heading
# @max: 32
text-title = Текст на знімку

# @where: Text panel status while the text is being read
# @kind: status
# @max: 40
text-busy = Розпізнаю текст…

# @where: Text panel status: how many lines were found (read on this device)
# @kind: status
# @max: 48
text-count = Рядків: { $n } · розпізнано на цьому пристрої

# @where: Text panel status: nothing was found
# @kind: status
# @max: 40
text-none = Тексту не знайдено

# @where: Text panel status: languages the system cannot read yet
# @kind: status
# @max: 80
text-missing = не встановлено для розпізнавання: { $langs }

# @where: Text panel status (Windows): no reader for Ukrainian — neither Znimok's helper nor the Windows language pack
# @kind: status
# @max: 200
text-missing-uk = української тут не прочитати, кирилиця може вийти хибною: перевстановіть Znimok (з ним іде розпізнавач тексту) або додайте в Windows мовний пакет «Українська»

# @where: Text panel status: the reading failed
# @kind: error
# @max: 96
text-error = Не вдалося розпізнати текст: { $error }

# @where: Text panel: copies the whole text as shown (after any corrections)
# @kind: button
# @max: 20
text-copy-all = Копіювати все

# @where: Text panel: hint under the text
# @kind: hint
# @max: 120
text-hint = Протягніть рамку по знімку — розпізнаю лише її; клік по рядку — скопіюю його.

# @where: Tray menu item: choose a part of the screen, its text goes to the clipboard
# @kind: menu
# @max: 40
tray-read-text = Скопіювати текст з екрана

# @where: Hotkeys settings row: choose a part of the screen, its text goes to the clipboard
# @kind: label
# @max: 40
keys-read-text = Скопіювати текст з екрана

# @where: Question after a quick text reading: the text is on the clipboard; the body shows it
# @kind: heading
# @max: 48
text-copied-title = Текст скопійовано — рядків: { $n }

# @where: Toast when the clipboard is locked or fails
# @kind: error
# @max: 80
clipboard-error = Не вдалося скопіювати в буфер обміну.

# @where: Footer of the self-contained HTML page of a screenshot; $version is the Znimok version
# @kind: hint
# @max: 48
html-made-with = Зроблено в Znimok { $version }

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

# @where: Tooltip of the card's trash button, after "Shift —": Shift+click skips the trash
# @kind: tooltip
# @max: 24
lib-delete-forever = видалити назавжди

# @where: Question before Shift+click deletes a document without the trash
# @kind: title
# @max: 40
lib-delete-forever-title = Видалити назавжди?

# @where: Body of that question; $name is the document's name
# @kind: body
# @max: 160
lib-delete-forever-body = «{ $name }» буде видалено, минаючи кошик. Скасувати це неможливо.

# @where: Status line after a document was deleted for good
# @kind: toast
# @max: 40
lib-deleted-forever-toast = Видалено назавжди

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

# @where: Row on the Hotkeys settings page (ZK-146)
# @kind: label
# @max: 32
keys-read-codes = Зчитати QR-коди з екрана

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

# @where: Settings, Screenshots: the gesture of a row (releasing the mouse without a modifier)
# @kind: label
# @max: 18
shots-gesture-plain = Відпустити

# @where: Settings, Screenshots: the gesture of a row (Shift held on release)
# @kind: label
# @max: 18
shots-gesture-shift = Shift + відпустити

# @where: Settings, Screenshots: the gesture of a row (Alt held on release; Windows)
# @kind: label
# @max: 18
shots-gesture-alt = Alt + відпустити

# @where: Settings, Screenshots: the gesture of a row (Option held on release; macOS)
# @kind: label
# @max: 18
shots-gesture-option = ⌥ + відпустити

# @where: Switch
# @kind: label
# @max: 48
shots-show-hints = Показувати смугу підказок під час знімання

# @where: Settings, Screenshots: segment — the capture opens in the editor window
# @kind: label
# @max: 16
shots-seg-editor = Редактор

# @where: Settings, Screenshots: segment — the capture opens in the editor right over the screen
# @kind: label
# @max: 16
shots-seg-over = Поверх екрану

# @where: Settings, Screenshots: segment — the capture goes straight to the clipboard
# @kind: label
# @max: 16
shots-seg-clipboard = У буфер

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

# @where: Settings → Recording: the switch — a finished recording opens in the editor (off: only the card after it)
# @kind: label
# @max: 44
rec-open-editor = Після запису відкривати редактор

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

# @where: Settings, Library: the row with the two fields for the words before the date in new names (ZK-221)
# @kind: label
# @max: 32
libset-names = Назви нових документів

# @where: Settings, Library: the label of the field with the word for new screenshots
# @kind: label
# @max: 16
libset-shot-prefix = Знімки

# @where: Settings, Library: the label of the field with the word for new recordings
# @kind: label
# @max: 16
libset-video-prefix = Записи

# @where: Settings, Library: the placeholder of the recordings' field — the usual word, as in rec-doc-name
# @kind: label
# @max: 16
libset-video-word = Запис

# @where: Settings, Library: the hint under the two fields
# @kind: body
# @max: 120
libset-names-hint = Слово перед датою й часом, як у «Знімок 2026-10-02 12.00.00». Порожнє — звичайне слово.

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

# @where: Library header: a segment — show everything
# @kind: button
# @max: 10
lib-kind-all = Усе

# @where: Library header: a segment — screenshots only
# @kind: button
# @max: 12
lib-kind-shots = Знімки

# @where: Library header: a segment — videos only
# @kind: button
# @max: 12
lib-kind-videos = Відео

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

# @where: About page, under the Znimok wordmark next to the app icon
# @kind: value
# @max: 32
about-version-line = Версія { $version }

# @where: About: after the version
# @kind: badge
# @max: 40
about-signed = підпис релізу перевірено

# @where: About: credits; Slint and font names stay as they are
# @kind: body
# @max: 200
about-made-with = Зроблено з Slint · шрифти Onest, JetBrains Mono, Unbounded (OFL) · ліцензії бібліотек

# @where: About page: the bundled fonts and their licence (keep the font names)
# @kind: body
# @max: 90
about-fonts = Шрифти: Onest, JetBrains Mono, Unbounded — SIL Open Font License 1.1

# @where: About page, under the Znimok wordmark: what the program is
# @kind: body
# @max: 70
about-tagline = Знімки й відео екрана з позначками

# @where: About page, next to the version: copies the version and build for a bug report
# @kind: button
# @max: 14
about-copy-version = Скопіювати

# @where: Toast after «Copy» on the About page
# @kind: body
# @max: 50
about-copied = Версію скопійовано

# @where: About page, after the version: $commit is 7 hex characters or «about-build-local»; $platform like «Windows x86_64»
# @kind: value
# @max: 60
about-build = збірка { $commit } · { $platform }

# @where: About page, under the version: the GPU that draws the window and plays video; $gpu like «Intel(R) UHD Graphics 630 (Dx12)»
# @kind: value
# @max: 80
about-gpu = Відеокарта: { $gpu }

# @where: About page: a build made on a developer's machine, not by the release pipeline
# @kind: value
# @max: 20
about-build-local = локальна

# @where: About page: what Znimok does, first paragraph
# @kind: body
# @max: 320
about-description = Znimok знімає екран — ділянку, вікно, увесь екран або довгу сторінку з прокручуванням — і записує відео. Стрілки, текст, лічильники, розмиття та інші позначки лишаються редагованими після збереження, текст зі знімка можна скопіювати як текст, а кожен знімок потрапляє в локальну бібліотеку.

# @where: About page: privacy in short, second paragraph (must match docs/privacy.md)
# @kind: body
# @max: 260
about-local = Усе лишається на вашому комп'ютері: без облікового запису, телеметрії й реклами. Без вашої дії нічого нікуди не йде; сама програма лише перевіряє оновлення, і це можна вимкнути.

# @where: About page: link button to the website
# @kind: button
# @max: 16
about-site = Сайт

# @where: About page: link button to the privacy policy
# @kind: button
# @max: 16
about-privacy = Приватність

# @where: About page: link button to the licence
# @kind: button
# @max: 16
about-licence = Ліцензія

# @where: About page: copyright line; the name in the language's script (uk: Вадим Слива), «Plum» as is
# @kind: body
# @max: 70
about-copyright = © 2026 Вадим Слива (Plum). Усі права захищено.

# @where: About page: under the copyright; what the public source means
# @kind: body
# @max: 200
about-licence-note = Вихідний код опубліковано, щоб кожен міг перевірити, що робить програма; це не дозвіл на його повторне використання. Умови — у «Ліцензії».

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

# @where: First-run guide, hotkeys card: the default combinations another program holds
# @kind: hint
# @max: 90
onb-keys-taken = Зайнято іншою програмою: { $keys }.

# @where: First-run guide, after onb-keys-taken: the region key that works instead
# @kind: hint
# @max: 60
onb-keys-fallback = Знімок ділянки поки працює на { $key }.

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

# @where: First-run guide, bottom left: check box, ticked by default
# @kind: option
# @max: 36
onb-dont-show = Не показувати наступного разу

# @where: Settings and the first-run guide
# @kind: hint
# @max: 80
autostart-needs-approval = Дозвольте Znimok у Системних параметрах → Загальні → Об'єкти входу

# @where: Settings and the first-run guide
# @kind: hint
# @max: 80
autostart-disabled-in-system = Вимкнено в Диспетчері задач — перемикач тут увімкне знову

# @where: Settings and the first-run guide
# @kind: hint
# @max: 80
onb-open-guide = Провідник першого запуску

## Crash reports
## After a crash Znimok offers its local report once at the next start. Nothing is sent automatically.


# @where: Title of the dialog shown at start after the previous run crashed
# @kind: title
# @max: 60
crash-title = Минулого разу Znimok неочікувано закрився

# @where: Text of the same dialog; $summary is the short technical reason (in English or empty)
# @kind: body
# @max: 300
crash-body = На цьому комп'ютері збережено звіт про збій: { $summary }. Можна відкрити заповнений issue на GitHub (ви переглянете й надішлете його самі) або подивитися файли звіту.

# @where: Button of the crash dialog: opens github.com with the report filled in
# @kind: button
# @max: 24
crash-open-issue = Відкрити issue на GitHub

# @where: Button of the crash dialog: shows the folder with the report files
# @kind: button
# @max: 24
crash-show-folder = Показати теку

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

# @where: Question title (macOS, no Screen Recording permission)
# @kind: title
# @max: 48
perm-missing-title = Немає дозволу на запис екрана

# @where: After the permission text: the alternative without it
# @kind: body
# @max: 200
perm-picker-hint = Або виберіть вікно чи екран у системному вікні macOS зараз — без дозволу, але macOS додасть на знімок свій значок трансляції.

# @where: Button: capture through the macOS content picker
# @kind: button
# @max: 28
perm-use-picker = Вибрати без дозволу

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

# @where: A permission of a connected agent
# @kind: label
# @max: 24
agents-scope-settings = налаштування

# @where: Switch: the local MCP server for agents on this computer
# @kind: label
# @max: 48
agents-access = Доступ агентам на цьому комп’ютері (MCP)

# @where: Under the switch: how an agent connects
# @kind: body
# @max: 160
agents-connect-hint = Claude Code: claude mcp add znimok -- znimok mcp. Claude Desktop: відкрийте файл .mcpb з релізу.

# @where: The action log has no entries
# @kind: body
# @max: 48
agents-log-empty = Поки нічого

# @where: A line of the action log; $when is a date and time
# @kind: body
# @max: 80
agents-log-entry = { $when } · { $client } · { $tool }

# @where: Button: show the action log file in its folder
# @kind: button
# @max: 24
agents-log-show = Показати файл

# @where: Status of the cloud assistant without a key
# @kind: value
# @max: 32
agents-cloud-off = вимкнено, ключа немає

# @where: Updates page: when the last check was; $when is a date or "never"
# @kind: value
# @max: 48
upd-last-check = Остання перевірка: { $when }

# @where: Updates page, instead of the last check when there was none
# @kind: value
# @max: 48
upd-never-checked = Перевірок ще не було

# @where: Updates page: button to check right now
# @kind: button
# @max: 24
upd-check-now = Перевірити зараз

# @where: Updates page: a check is running
# @kind: status
# @max: 32
upd-checking = Перевіряю…

# @where: Updates page: the installer is being downloaded and verified
# @kind: status
# @max: 60
upd-downloading = Завантажую й перевіряю оновлення…

# @where: Updates page (macOS): Sparkle unpacks the downloaded update
# @kind: status
# @max: 40
upd-extracting = Розпаковую оновлення…

# @where: Updates page (macOS): Sparkle installs and relaunches the app
# @kind: status
# @max: 48
upd-installing = Встановлюю й перезапускаю…

# @where: Updates page: no newer release
# @kind: status
# @max: 40
upd-up-to-date = У вас остання версія

# @where: Updates page: this build has no release key yet
# @kind: status
# @max: 80
upd-not-configured = У цій збірці оновлення ще не налаштовані

# @where: Updates page: the check or the download failed; $reason is technical
# @kind: status
# @max: 160
upd-failed = Не вдалося оновити: { $reason }

# @where: Updates page (macOS until Sparkle): open the release on GitHub
# @kind: button
# @max: 24
upd-release-page = Сторінка релізу

# @where: Title of the note about the last update, shown once after it
# @kind: title
# @max: 40
upd-outcome-title = Оновлення

# @where: The note after an update: it worked; $version is the new version
# @kind: body
# @max: 60
upd-outcome-installed = Znimok оновлено до { $version }

# @where: The note after an update: the new version did not start, the previous one is back; $reason from the installer
# @kind: body
# @max: 160
upd-outcome-rolled-back = { $version } не запустилась ({ $reason }) — повернуто попередню версію

# @where: The note after an update that failed; $reason from the installer
# @kind: body
# @max: 120
upd-outcome-failed = Оновлення не вдалося: { $reason }

# @where: Updates page: where updates come from
# @kind: body
# @max: 160
upd-channel-hint = Релізи на GitHub із підписом; перед установкою перевіряється підпис.

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

# @where: Transport: plays the video backwards (J)
# @kind: a11y
# @max: 18
vid-play-back = Пуск назад

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

## For developers
## A temporary settings page for testing (owner, 29.09).


# @where: Settings → For developers
# @kind: button
# @max: 60
dev-page = Для розробника

# @where: Settings → For developers: the note at the top of the page
# @kind: body
# @max: 120
dev-intro = Тимчасова сторінка для перевірок: скинути стан і показати те, що зазвичай з'являється лише раз.

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-onboarding = Показати провідник першого запуску знову

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-pill = Показати плашку після знімка

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-crash = Показати питання про звіт збою

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-open-settings = Відкрити теку налаштувань

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-open-logs = Відкрити теку журналів

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-empty-trash = Очистити кошик бібліотеки

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-reset = Скинути всі налаштування

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-done = Готово

## Library trash and picking cards (ZK-175, ZK-176)


# @where: Library header: the switch to the library's trash
# @kind: tab
# @max: 16
lib-trash-tab = Кошик

# @where: Library → Trash header: delete everything in the trash for good
# @kind: button
# @max: 24
trash-destroy-all = Знищити все

# @where: Library → Trash: card button and the picked bar; back into the library
# @kind: button
# @max: 16
trash-restore = Відновити

# @where: Library → Trash: card button, the picked bar and the question's button; delete for good
# @kind: button
# @max: 16
trash-destroy = Знищити

# @where: Library → Trash, nothing in it
# @kind: title
# @max: 40
trash-empty-title = Кошик порожній

# @where: Library → Trash, nothing in it; $days is the setting
# @kind: body
# @max: 160
trash-empty-body = Видалені документи лежать тут { $days } дн., потім зникають назавжди. Термін — у Налаштуваннях → Бібліотека.

# @where: Library: the bar over picked cards; $count cards
# @kind: label
# @max: 24
lib-picked = Вибрано: { $count }

# @where: Library: the bar over picked cards, the close button
# @kind: tooltip
# @max: 32
lib-pick-none = Зняти вибір

# @where: Settings → Library: the days a deleted document stays in the trash
# @kind: label
# @max: 60
libset-trash-days = Зберігати видалені документи в кошику, днів

# @where: Question before deleting from the trash for good
# @kind: title
# @max: 40
trash-destroy-title = Знищити назавжди?

# @where: Toast after several documents went to the trash, with Undo
# @kind: toast
# @max: 48
lib-trashed-many-toast =
    { $count ->
        [one] { $count } документ переміщено в кошик
        [few] { $count } документи переміщено в кошик
        [many] { $count } документів переміщено в кошик
       *[other] { $count } документи переміщено в кошик
    }

# @where: Toast after documents came back from the trash
# @kind: toast
# @max: 48
trash-restored-toast =
    { $count ->
        [one] Відновлено { $count } документ
        [few] Відновлено { $count } документи
        [many] Відновлено { $count } документів
       *[other] Відновлено { $count } документи
    }

# @where: Body of the question before deleting from the trash; $count documents
# @kind: body
# @max: 160
trash-destroy-body =
    { $count ->
        [one] { $count } документ буде видалено назавжди. Скасувати це неможливо.
        [few] { $count } документи буде видалено назавжди. Скасувати це неможливо.
        [many] { $count } документів буде видалено назавжди. Скасувати це неможливо.
       *[other] { $count } документи буде видалено назавжди. Скасувати це неможливо.
    }

# @where: Library → Trash: a card's line; $date when it went in, $days until it goes for good
# @kind: label
# @max: 48
trash-card-meta =
    { $days ->
        [one] видалено { $date } · зникне за { $days } день
        [few] видалено { $date } · зникне за { $days } дні
        [many] видалено { $date } · зникне за { $days } днів
       *[other] видалено { $date } · зникне за { $days } дні
    }

## Library groups, pins and keys (ZK-177, ZK-178, ZK-179)


# @where: Library card button: pin the document
# @kind: tooltip
# @max: 60
lib-pin = Закріпити — ліміт бібліотеки його не прибере

# @where: Library: the bar over picked cards, pin them
# @kind: button
# @max: 16
lib-pin-short = Закріпити

# @where: Library: unpin (card button and the bar over picked cards)
# @kind: button
# @max: 16
lib-unpin = Відкріпити

# @where: Library grid: title of the pinned documents' group
# @kind: title
# @max: 24
lib-group-pinned = Закріплені

# @where: Library grid: group title
# @kind: title
# @max: 24
lib-group-today = Сьогодні

# @where: Library grid: group title
# @kind: title
# @max: 24
lib-group-yesterday = Вчора

# @where: Library grid: group title, earlier this week
# @kind: title
# @max: 24
lib-group-week = Цього тижня

# @where: Library grid: group title, earlier this month
# @kind: title
# @max: 24
lib-group-month = Цього місяця

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-1 = Січень

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-2 = Лютий

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-3 = Березень

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-4 = Квітень

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-5 = Травень

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-6 = Червень

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-7 = Липень

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-8 = Серпень

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-9 = Вересень

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-10 = Жовтень

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-11 = Листопад

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-12 = Грудень

## Video mode of the editor (ZK-181)


# @where: Inspector tab for a video document, in place of «Image»
# @kind: tab
# @max: 12
insp-tab-video = Відео

# @where: Video transport: the start of the kept video is the current frame (I)
# @kind: button
# @max: 16
vid-in-here = Початок тут

# @where: Video transport: the end of the kept video is the current frame (O)
# @kind: button
# @max: 16
vid-out-here = Кінець тут

# @where: Video transport: split the strip at the current frame (S)
# @kind: button
# @max: 16
vid-split-here = Розрізати тут

# @where: Video timeline: cut the selected piece (Del); also the row «Cut» in the Trim section
# @kind: button
# @max: 16
vid-cut = Вирізати

# @where: Video timeline: cut everything but the selected piece (Shift+Del)
# @kind: button
# @max: 24
vid-keep-only = Лишити лише це

# @where: Video timeline: a cut-out piece comes back
# @kind: button
# @max: 16
vid-restore = Повернути

# @where: Video, Trim section: forget every trim and cut
# @kind: button
# @max: 24
vid-reset-trim = Скинути обрізання

# @where: Video, Trim section: how long the video is after the edits
# @kind: label
# @max: 16
vid-left = Лишається

# @where: Video, Trim section: a note under the numbers
# @kind: body
# @max: 160
vid-trim-note = Запис не змінюється: вирізане можна повернути, а файл переписується лише при експорті.

# @where: Video, Trim section: how the strip is used
# @kind: body
# @max: 120
vid-tl-hint = Тягніть по стрічці — фрагмент · Del вирізає · Shift+Del лишає тільки його · S розрізає

# @where: Video, Sound section: no track goes into the export
# @kind: button
# @max: 16
vid-no-sound = Без звуку

# @where: Video, Sound section: both tracks go into the export
# @kind: button
# @max: 12
vid-sound-both = Обидва

# @where: Video transport: the whole timeline in view
# @kind: button
# @max: 12
vid-fit = Вписати

# @where: Video, Trim section: the player could not start on this machine
# @kind: body
# @max: 160
vid-poster-note = Відео не вдалося відтворити тут: перший кадр стоїть за нього; обрізання працює.

# @where: Name of the screenshot made from a video frame; $name the video, $n the frame
# @kind: label
# @max: 60
vid-frame-doc-name = { $name } — кадр { $n }

# @where: Video, Trim section: how many pieces are cut out and their length; $count, $seconds
# @kind: label
# @max: 40
vid-cut-count =
    { $count ->
        [one] { $count } фрагмент · { $seconds } с
        [few] { $count } фрагменти · { $seconds } с
        [many] { $count } фрагментів · { $seconds } с
       *[other] { $count } фрагменти · { $seconds } с
    }

# @where: Video transport: the timeline shows more time
# @kind: tooltip
# @max: 24
vid-zoom-out = Таймлайн: дрібніше

# @where: Video transport: the timeline shows less time, larger
# @kind: tooltip
# @max: 24
vid-zoom-in = Таймлайн: крупніше

# @where: Video, Sound section: put a muted track back into the export
# @kind: button
# @max: 12
vid-track-on = Увімкнути

# @where: Video, Sound section: leave a track out of the export
# @kind: button
# @max: 12
vid-track-off = Вимкнути

## Recording in the app (ZK-180, ZK-91)


# @where: Settings → Hotkeys: the row of the recording hotkey
# @kind: label
# @max: 40
keys-video = Почати / зупинити запис відео

# @where: Tray menu while recording, followed by the time
# @kind: menu
# @max: 24
tray-stop-record = Зупинити запис

# @where: Settings → Recording: how a recording starts and ends
# @kind: body
# @max: 240
rec-how = Клавіша запису або «Записати відео» в треї відкривають ту саму накладку, що й для знімків: потягнути — ділянка, клік — вікно, Пробіл — увесь екран. Та сама клавіша або «Стоп» завершують; відео йде в бібліотеку.

# @where: A recording could not start; $reason from the system
# @kind: toast
# @max: 160
rec-error-start = Не вдалося почати запис: { $reason }

# @where: A finished recording could not be saved to the library; $reason
# @kind: toast
# @max: 160
rec-error-save = Не вдалося зберегти запис: { $reason }

# @where: Settings → Recording, under «Cursor and click highlight»
# @kind: hint
# @max: 260
rec-cursor-hint = Вказівник малюється у відео, клік — кільцем, що розходиться, утримана кнопка — сталим; кліки по плашці запису не записуються. Кліки також зберігаються журналом у документі.

# @where: Settings → Recording, under the sound choice
# @kind: hint
# @max: 260
rec-sound-tracks = Кожне джерело — окрема доріжка: у редакторі її можна вимкнути чи зробити тихішою. Системний звук — те, що відтворює цей комп'ютер; мікрофону потрібен дозвіл Windows (Конфіденційність → Мікрофон).

# @where: Toast after a recording: Windows privacy settings deny the microphone
# @kind: status
# @max: 200
rec-warn-mic-denied = Записано без мікрофона: Windows його не дозволяє (Параметри → Конфіденційність → Мікрофон → класичні програми).

# @where: Toast after a recording: another program holds the sound device exclusively
# @kind: status
# @max: 200
rec-warn-audio-busy = Записано без частини звуку: пристрій звуку зайняла інша програма.

# @where: Toast after a recording: a sound source could not be opened (no device, or the encoder took no audio)
# @kind: status
# @max: 200
rec-warn-audio-none = Записано без частини звуку: не вдалося відкрити пристрій звуку.

# @where: Name of a new recording in the library; $date, $time
# @kind: label
# @max: 40
rec-doc-name = Запис { $date } { $time }

# @where: Recording is not available on this system yet (macOS until the next update)
# @kind: toast
# @max: 120
rec-not-here = Запис на цій системі — з наступним оновленням.

# @where: A recording ended without a single frame (a window that never changed)
# @kind: toast
# @max: 120
rec-nothing = Нічого не записано: вікно не змінювалося, поки його записували.

## The «Відео» tab (ZK-188)


# @where: Crop panel: free proportions (the other presets are ratios like 16:9)
# @kind: button
# @max: 10
crop-aspect-free = Вільно

# @where: Video tab: section title, the playback speed
# @kind: label
# @max: 24
vid-speed-title = Швидкість

# @where: Video tab: under the speed buttons
# @kind: body
# @max: 120
vid-speed-note = Лише для відтворення в редакторі; експорт зберігає справжню швидкість.

# @where: Video tab: section title, the width and height of the exported video
# @kind: label
# @max: 30
vid-out-title = Розмір на виході

# @where: Video tab: under the size on export
# @kind: body
# @max: 160
vid-out-note = Кадр масштабується при експорті; запис лишається як є. Сторони парні — так треба кодувальнику.

## The browser log (ZK-97)


# @where: Settings → Recording: section title, the browser log
# @kind: label
# @max: 24
rec-browser = Браузер

# @where: Settings → Recording: switch, the DevTools log of a recording
# @kind: label
# @max: 70
rec-devlog = Писати лог DevTools браузера разом із записом

# @where: Settings → Recording: switch, the extension may start recordings
# @kind: label
# @max: 70
rec-ext-control = Дозволити розширенню починати запис свого вікна

# @where: Settings → Recording: the extension is connected; $count browsers
# @kind: body
# @max: 80
rec-browsers-on =
    { $count ->
        [one] Розширення підключено ({ $count } браузер).
        [few] Розширення підключено ({ $count } браузери).
        [many] Розширення підключено ({ $count } браузерів).
       *[other] Розширення підключено ({ $count } браузера).
    }

# @where: Settings → Recording: no browser with the extension is connected
# @kind: body
# @max: 90
rec-browsers-off = Жоден браузер із розширенням Znimok не підключено.

# @where: Settings → Recording: what the browser log writes and how to install the extension
# @kind: body
# @max: 500
rec-devlog-hint = Розширення Znimok для Chrome і Edge пише консоль сторінки, помилки, мережу (заголовки, тіла запитів і відповідей) і переходи синхронно з відео — усе, що показують панелі DevTools, для налагодження; чутливе можна приховати при експорті. Встановіть його з Chrome Web Store (кнопка нижче).

# @where: Settings → Recording: the button under the extension's hint; opens its Chrome Web Store page
# @kind: button
# @max: 24
rec-devlog-store = Chrome Web Store

## Video export (ZK-190)


# @where: The header button and menu of a video: the MP4 file to the clipboard
# @kind: button
# @max: 18
vid-copy-mp4 = Копіювати MP4

# @where: Share menu of a video: a GIF file to the clipboard
# @kind: menu
# @max: 24
vid-copy-gif = Копіювати GIF

# @where: Share menu of a video: the frame shown with its marks, as a picture
# @kind: menu
# @max: 24
vid-copy-frame = Копіювати кадр

# @where: Share menu of a video: the exported MP4 as a new library document
# @kind: menu
# @max: 30
vid-save-library = Зберегти в бібліотеку

# @where: Video export sheet: the MP4 card title
# @kind: label
# @max: 10
vexp-mp4 = MP4

# @where: Video export sheet: the MP4 card
# @kind: body
# @max: 90
vexp-mp4-desc = Грає в будь-якому програвачі й чаті; звук зведено в одну доріжку.

# @where: Video export sheet: the GIF card title
# @kind: label
# @max: 10
vexp-gif = GIF

# @where: Video export sheet: the GIF card, its width and rate; $w, $fps
# @kind: label
# @max: 24
vexp-gif-sub = { $w } px · { $fps } к/с

# @where: Video export sheet: the GIF card
# @kind: body
# @max: 90
vexp-gif-desc = Грає будь-де без програвача; без звуку, 255 кольорів.

# @where: Video export sheet: the HTML card title
# @kind: label
# @max: 10
vexp-html = HTML

# @where: Video export sheet: the HTML card, under its title
# @kind: label
# @max: 20
vexp-html-sub = одна сторінка

# @where: Video export sheet: the HTML card
# @kind: body
# @max: 90
vexp-html-desc = Відео з позначками живим шаром; відкривається в будь-якому браузері.

# @where: Export sheet, the HTML card's subtitle when the recording has the browser's log (ZK-226)
# @kind: label
# @max: 28
vexp-html-sub-log = одна сторінка з логом

# @where: Export sheet, the HTML card's description when the recording has the browser's log
# @kind: body
# @max: 90
vexp-html-desc-log = Відео, позначки й лог DevTools на одній сторінці; відкривається в будь-якому браузері.

# @where: Export sheet, under the HTML card when the recording has the browser's log
# @kind: body
# @max: 160
vexp-html-log-note = Сторінка несе лог DevTools браузера поруч із відео, як і звіт; чутливе приховується, як вибрано нижче.

# @where: Video export sheet: the frame-as-screenshot card
# @kind: body
# @max: 90
vexp-frame-desc = Поточний кадр із позначками — новим знімком.

# @where: Video export sheet: the card of the report with the browser log (not yet)
# @kind: label
# @max: 40
vexp-report = Звіт з логом DevTools

# @where: Video export sheet: a card not available yet
# @kind: label
# @max: 20
vexp-soon = згодом

# @where: Video export sheet: the report card (not yet)
# @kind: body
# @max: 90
vexp-report-desc = Відео, позначки й лог DevTools браузера на одній сторінці — для звіту про ваду.

# @where: Video export sheet, GIF: the size limit label
# @kind: label
# @max: 24
vexp-limit = Не більше ніж

# @where: Video export sheet, GIF: no size limit
# @kind: label
# @max: 16
vexp-limit-off = без межі

# @where: Video export sheet, GIF: under the size limit
# @kind: body
# @max: 140
vexp-limit-note = Якщо більше — Znimok знизить частоту кадрів, потім ширину, і скаже, що змінив.

# @where: Video export sheet: what the MP4 export does
# @kind: body
# @max: 200
vexp-mp4-note = H.264 апаратним кодеком; увімкнені звукові доріжки зводяться в одну; вирізане пропускається з короткими згасаннями на стиках.

# @where: Video export sheet: what the HTML export does
# @kind: body
# @max: 200
vexp-html-note = Відео й позначки в одному файлі: позначки — шар над відео, у свій час; приховування й маркер — у самому відео.

# @where: Video export sheet: what the frame card does
# @kind: body
# @max: 160
vexp-frame-note = Кадр із полотна в повному розмірі з видимими позначками — новий знімок в окремому вікні.

# @where: Video export sheet: the clipboard gets a file
# @kind: body
# @max: 120
vexp-clipboard-note = У буфер іде файл — вставте його в чат або теку.

# @where: Video export sheet: progress; $pct
# @kind: label
# @max: 30
vexp-working = Експорт… { $pct } %

# @where: Toast when a quick export starts; $format
# @kind: toast
# @max: 60
vexp-working-toast = Готую { $format }…

# @where: Toast: an exported video file is on the clipboard; $format
# @kind: toast
# @max: 90
vexp-copied = { $format } скопійовано — вставте в чат або теку

# @where: Toast: the exported MP4 is a new library document
# @kind: toast
# @max: 60
vexp-library-done = MP4 — у бібліотеці

# @where: Toast addition: what the GIF limit changed; $w, $fps
# @kind: toast
# @max: 60
vexp-lowered = щоб вмістилось: { $w } px, { $fps } к/с

# @where: Toast: the export was cancelled
# @kind: toast
# @max: 40
vexp-cancelled = Експорт скасовано

# @where: Toast: the export failed; $reason
# @kind: toast
# @max: 120
vexp-error = Не вдалося експортувати: { $reason }

# @where: Video export sheet header: kept length, size, what applies; $time, $w, $h, $what
# @kind: label
# @max: 90
vexp-subtitle = { $time } після обрізання · { $w } × { $h } { $what }

# @where: Video export sheet header: marks and the frame are applied
# @kind: label
# @max: 40
vexp-applied-both = · позначки й кадр застосовано

# @where: Video export sheet header: marks are applied
# @kind: label
# @max: 30
vexp-applied-marks = · позначки застосовано

# @where: Video export sheet header: the frame (crop / size) is applied
# @kind: label
# @max: 30
vexp-applied-frame = · кадр застосовано

## The DevTools log panel (ZK-191)


# @where: Video timeline: the DevTools log lane title (short, lower case like the other lanes)
# @kind: label
# @max: 10
vid-lane-log = лог

# @where: Video timeline: tooltip, opens the DevTools log panel
# @kind: label
# @max: 40
devp-show = Показати лог

# @where: Video timeline: tooltip, hides the DevTools log panel
# @kind: label
# @max: 40
devp-hide = Сховати лог

# @where: Video transport: button, jumps to the next error of the browser log
# @kind: label
# @max: 40
devp-next-error = До наступної помилки

# @where: DevTools log panel: search field placeholder
# @kind: label
# @max: 30
devp-search = Пошук у лозі

# @where: DevTools log panel: no row matches the search and filter
# @kind: body
# @max: 60
devp-empty = Нічого не знайдено.

# @where: DevTools log panel: button, saves a response body to a file
# @kind: label
# @max: 24
devp-save = Зберегти як…

# @where: Toast: a response body was saved; $name the file name
# @kind: body
# @max: 80
devp-saved = Збережено: { $name }

# @where: Toast: a response body could not be saved
# @kind: body
# @max: 80
devp-save-failed = Не вдалося зберегти файл.

# @where: DevTools log panel: tab of a request (as in Chrome DevTools)
# @kind: label
# @max: 14
devp-tab-headers = Заголовки

# @where: DevTools log panel: tab, the request body (as in Chrome DevTools)
# @kind: label
# @max: 14
devp-tab-payload = Тіло запиту

# @where: DevTools log panel: tab, the response formatted (as in Chrome DevTools)
# @kind: label
# @max: 14
devp-tab-preview = Перегляд

# @where: DevTools log panel: tab, the raw response (as in Chrome DevTools)
# @kind: label
# @max: 14
devp-tab-response = Відповідь

# @where: DevTools log panel: tab, the request phases (as in Chrome DevTools)
# @kind: label
# @max: 14
devp-tab-timing = Час

# @where: DevTools log panel: filter chip, every event (a count follows)
# @kind: label
# @max: 16
devp-chip-all = Усі

# @where: DevTools log panel: filter chip, errors (a count follows)
# @kind: label
# @max: 16
devp-chip-errors = Помилки

# @where: DevTools log panel: filter chip, warnings (a count follows)
# @kind: label
# @max: 16
devp-chip-warnings = Попередження

# @where: DevTools log panel: filter chip, requests and WebSocket frames (a count follows)
# @kind: label
# @max: 16
devp-chip-network = Мережа

# @where: DevTools log panel: filter chip, console messages and exceptions (a count follows)
# @kind: label
# @max: 16
devp-chip-console = Консоль

# @where: DevTools log panel: filter chip, page navigations (a count follows)
# @kind: label
# @max: 16
devp-chip-nav = Переходи

# @where: DevTools log panel, Headers tab: section title (as «General» in Chrome DevTools)
# @kind: label
# @max: 30
devp-general = Загальне

# @where: DevTools log panel, Headers tab: section title
# @kind: label
# @max: 30
devp-res-headers = Заголовки відповіді

# @where: DevTools log panel, Headers tab: section title
# @kind: label
# @max: 30
devp-req-headers = Заголовки запиту

# @where: DevTools log panel, Headers tab: field, the request address
# @kind: label
# @max: 20
devp-h-url = Адреса

# @where: DevTools log panel, Headers tab: field, GET / POST…
# @kind: label
# @max: 20
devp-h-method = Метод

# @where: DevTools log panel, Headers tab: field, the HTTP status
# @kind: label
# @max: 20
devp-h-status = Статус

# @where: DevTools log panel, Headers tab: field, the server IP address
# @kind: label
# @max: 20
devp-h-remote = Адреса сервера

# @where: DevTools log panel, Headers tab: field, h2 / http/1.1
# @kind: label
# @max: 20
devp-h-protocol = Протокол

# @where: DevTools log panel, Headers tab: field, what started the request
# @kind: label
# @max: 20
devp-h-initiator = Ініціатор

# @where: DevTools log panel, Headers tab: field, served from the cache
# @kind: label
# @max: 20
devp-h-cache = З кешу

# @where: DevTools log panel, Headers tab: field, why the request failed
# @kind: label
# @max: 20
devp-h-error = Помилка

# @where: DevTools log panel: title above a call stack
# @kind: label
# @max: 20
devp-stack = Стек викликів

# @where: DevTools log panel: under a long text, only its start is shown
# @kind: body
# @max: 120
devp-cut = Показано лише перші 256 КБ; «Зберегти як…» збереже все.

# @where: DevTools log panel: a binary response; $size its size
# @kind: body
# @max: 120
devp-binary = Двійкові дані, { $size }. «Зберегти як…» збереже їх.

# @where: DevTools log panel: an event without details
# @kind: body
# @max: 40
devp-nothing = Даних немає.

# @where: DevTools log panel, Timing tab: phase, waiting before the request (as «Queueing» in DevTools)
# @kind: label
# @max: 20
devp-t-queue = Черга

# @where: DevTools log panel, Timing tab: phase, the DNS lookup
# @kind: label
# @max: 20
devp-t-dns = Пошук DNS

# @where: DevTools log panel, Timing tab: phase, the connection
# @kind: label
# @max: 20
devp-t-connect = З’єднання

# @where: DevTools log panel, Timing tab: phase, the TLS handshake
# @kind: label
# @max: 20
devp-t-tls = TLS

# @where: DevTools log panel, Timing tab: phase, sending the request
# @kind: label
# @max: 20
devp-t-send = Надсилання

# @where: DevTools log panel, Timing tab: phase, waiting for the server (TTFB)
# @kind: label
# @max: 20
devp-t-wait = Очікування (TTFB)

# @where: DevTools log panel, Timing tab: phase, receiving the response
# @kind: label
# @max: 20
devp-t-download = Завантаження

# @where: DevTools log panel, Timing tab: the whole request
# @kind: label
# @max: 20
devp-t-total = Усього

## The developer report (ZK-98)


# @where: Export sheet, the developer report: what the two forms are
# @kind: body
# @max: 240
vexp-report-note = Одна сторінка до 100 МБ відкривається в будь-якому браузері; .zreport тримає відео поруч зі сторінкою, а лог і dataLayer — у JSON, і відкривається в Znimok як запис.

# @where: Export sheet, the developer report: form option, one HTML page
# @kind: label
# @max: 22
vexp-report-html = Одна сторінка (HTML)

# @where: Export sheet, the developer report: form option, a ZIP archive
# @kind: label
# @max: 22
vexp-report-zip = Архів (.zreport)

# @where: Export sheet, the developer report: the estimate is over 100 MB as one page
# @kind: body
# @max: 100
vexp-report-big = Понад 100 МБ однією сторінкою — оберіть .zreport.

# @where: Export sheet, the developer report: the recording has no browser log
# @kind: body
# @max: 120
vexp-no-log = У цього запису немає логу браузера: у звіті будуть відео і позначки.

# @where: Export sheet, the developer report: switch, hide sensitive values of the log
# @kind: label
# @max: 40
vexp-hide = Приховати чутливе

# @where: Export sheet, the developer report: Settings say always hide
# @kind: body
# @max: 100
vexp-hide-always = Чутливе приховується завжди (Налаштування → Запис).

# @where: Export sheet, the developer report: Settings say never hide
# @kind: body
# @max: 100
vexp-hide-never = Чутливе не приховується (Налаштування → Запис).

# @where: Export sheet, the developer report: how many values will be hidden; $count
# @kind: body
# @max: 200
vexp-hide-count =
    { $count ->
        [one] Буде приховано { $count } значення: ключі зі списку в Налаштуваннях і секрети за їхнім виглядом.
        [few] Буде приховано { $count } значення: ключі зі списку в Налаштуваннях і секрети за їхнім виглядом.
        [many] Буде приховано { $count } значень: ключі зі списку в Налаштуваннях і секрети за їхнім виглядом.
       *[other] Буде приховано { $count } значення: ключі зі списку в Налаштуваннях і секрети за їхнім виглядом.
    }

# @where: Export sheet, the developer report: nothing sensitive was found in the log
# @kind: body
# @max: 60
vexp-hide-none = У лозі не знайдено чутливого.

# @where: Toast: the report is over 100 MB as one page
# @kind: body
# @max: 100
vexp-report-too-big = Звіт однією сторінкою більший за 100 МБ — експортуйте його як .zreport.

# @where: Developer report page: header fact, when it was recorded
# @kind: label
# @max: 20
report-recorded = Записано

# @where: Developer report page: header fact, the video length
# @kind: label
# @max: 20
report-length = Тривалість

# @where: Developer report page: header fact, the video size in pixels
# @kind: label
# @max: 20
report-size = Розмір

# @where: Developer report page: header fact, the browser
# @kind: label
# @max: 20
report-browser = Браузер

# @where: Developer report page: header fact, the page address the log starts on
# @kind: label
# @max: 20
report-page = Сторінка

# @where: Developer report page: how many values of the log were hidden; $n a number
# @kind: body
# @max: 60
report-masked = Приховано значень: { $n }

# @where: Developer report page: footer; $version the app version
# @kind: body
# @max: 60
report-foot = Зроблено в Znimok { $version }

# @where: Developer report page: the recording has no browser log
# @kind: body
# @max: 80
report-no-log = У цього запису немає логу браузера.

# @where: Report page (the browser's viewer): the chip that filters the clicks of the recording
# @kind: label
# @max: 12
report-chip-clicks = Кліки

# @where: Report page: the kind column of a click row
# @kind: label
# @max: 12
report-click = Клік

# @where: Report page: the keyboard keys, at the bottom right
# @kind: body
# @max: 120
report-keys = ↑ ↓ — події · E — наступна помилка · Пробіл — пуск/пауза · ← → — ±2 с · / — пошук · Esc — закрити

# @where: Toast: a .zreport could not be opened; $reason why
# @kind: body
# @max: 120
zreport-error = Не вдалося відкрити звіт: { $reason }

# @where: Settings → Recording: section, hiding values in a developer report
# @kind: label
# @max: 50
rec-hide-title = Приховувати чутливе у звіті

# @where: Settings → Recording: option, the export sheet asks
# @kind: label
# @max: 14
rec-hide-ask = Питати

# @where: Settings → Recording: option, always hide
# @kind: label
# @max: 14
rec-hide-always = Завжди

# @where: Settings → Recording: option, never hide
# @kind: label
# @max: 14
rec-hide-never = Ніколи

# @where: Settings → Recording: label of the field with the keys to hide
# @kind: label
# @max: 30
rec-hide-keys = Ключі, які приховувати

# @where: Settings → Recording: what the keys are and what else is hidden
# @kind: body
# @max: 300
rec-hide-hint = Через кому: заголовки, ключі JSON, поля форм і ключі dataLayer, значення яких стануть •••. Секрети на кшталт токенів, адрес пошти й номерів карток знаходяться і за виглядом. У ваших файлах лог лишається цілим.

## dataLayer in the DevTools log (ZK-195)


# @where: DevTools log panel: filter chip, values pushed into dataLayer by GTM / gtag (a count follows)
# @kind: label
# @max: 16
devp-chip-datalayer = dataLayer

# @where: DevTools log panel: a dataLayer value that was already there when the recording started
# @kind: body
# @max: 120
devp-dl-pre = Уже був у dataLayer, коли почався запис.

# @where: DevTools log panel: a dataLayer value from a frame inside the page
# @kind: body
# @max: 80
devp-dl-frame = З фрейму всередині сторінки.

## The search by text on screenshots (ZK-186)


# @where: Settings → Library: switch, search the text on screenshots too
# @kind: label
# @max: 50
libset-search-text = Шукати й за текстом на знімках

# @where: Settings → Library: what the search by text does and keeps
# @kind: body
# @max: 300
libset-search-text-hint = Znimok читає текст на ваших знімках на цьому комп'ютері, поволі у фоні, і тримає його лише в локальному індексі бібліотеки — не у файлах і ніде в мережі. Якщо вимкнути, прочитане забувається.

# @where: Settings → Library: progress of reading; $done of $count screenshots
# @kind: body
# @max: 80
libset-text-reading = Читаю текст: { $done } з { $count }…

# @where: Settings → Library: all screenshots are read; $count of them
# @kind: body
# @max: 80
libset-text-ready =
    { $count ->
        [one] Текст { $count } знімка доступний для пошуку.
        [few] Текст { $count } знімків доступний для пошуку.
        [many] Текст { $count } знімків доступний для пошуку.
       *[other] Текст { $count } знімка доступний для пошуку.
    }

# @where: Report page: the first tab of an event's details (not a network request)
# @kind: label
# @max: 16
report-tab-details = Подробиці

# @where: Report page: the details pane while no event is chosen
# @kind: body
# @max: 60
report-pick = Виберіть подію, щоб побачити подробиці

# @where: Report page: the button that copies what the details show
# @kind: button
# @max: 14
report-copy = Копіювати

# @where: Report page, a request's details: the resource type and its MIME type
# @kind: label
# @max: 16
report-h-type = Тип

# @where: Report page, a request's details: bytes transferred
# @kind: label
# @max: 16
report-h-size = Розмір

# @where: Report page, a console message's details: the script and line it came from
# @kind: label
# @max: 16
report-h-source = Джерело

# @where: Report page, a tab's details: the page title
# @kind: label
# @max: 16
report-h-title = Назва

# @where: Developer report page: header fact, who made the recording (name, contact)
# @kind: label
# @max: 20
report-author = Запис створено

# @where: Settings → Recording: heading of the fields that sign a developer report
# @kind: label
# @max: 40
rec-sign-title = Підпис звіту

# @where: Settings → Recording, report signature: placeholder of the name field
# @kind: placeholder
# @max: 30
rec-sign-name = Ваше ім'я

# @where: Settings → Recording, report signature: placeholder of the contact field
# @kind: placeholder
# @max: 40
rec-sign-contact = E-mail чи інший контакт

# @where: Settings → Recording, report signature: placeholder of the rights notice field
# @kind: placeholder
# @max: 70
rec-sign-rights = Застереження про права, напр. © 2026 Компанія. Конфіденційно.

# @where: Settings → Recording, report signature: what the fields are for
# @kind: body
# @max: 200
rec-sign-hint = Сторінка звіту вгорі каже, хто зробив запис, а внизу — кому належить зображене. Лишіть поля порожніми, щоб звіт був без підпису.

# @where: Video export sheet, a page with the browser log: label of the page's language choice
# @kind: label
# @max: 24
vexp-lang = Мова сторінки

# @where: Video export sheet, a page with the browser log: switch, add the signature from the settings
# @kind: label
# @max: 50
vexp-sign = Підписати: хто записав, застереження про права

# @where: Video export sheet, a page with the browser log: no signature is set
# @kind: body
# @max: 100
vexp-sign-none = Своє ім'я й застереження про права можна додати в Налаштуваннях → Запис.

# @where: Title of the question when an AI agent wants access; $client is the name the agent reports
# @kind: title
# @max: 60
agents-ask-title = «{ $client }» просить доступ

# @where: Body of the access question; $what is one of agents-ask-capture…, $tool the tool's name
# @kind: body
# @max: 260
agents-ask-body = ШІ-агент, що називає себе «{ $client }», хоче { $what }. Запит прийшов через «{ $tool }». Назву програма повідомляє про себе сама — дозволяйте лише те, що запустили ви.

# @where: Inside agents-ask-body after «wants to»: screenshots
# @kind: body
# @max: 80
agents-ask-capture = робити знімки екрана й бачити список відкритих вікон

# @where: Inside agents-ask-body after «wants to»: reading the library
# @kind: body
# @max: 80
agents-ask-library-read = читати документи вашої бібліотеки й експортувати копії

# @where: Inside agents-ask-body after «wants to»: changing the library
# @kind: body
# @max: 80
agents-ask-library-write = змінювати документи вашої бібліотеки й додавати нові

# @where: Inside agents-ask-body after «wants to»: settings
# @kind: body
# @max: 80
agents-ask-settings = читати й змінювати налаштування Znimok

# @where: Inside agents-ask-body after «wants to»: an access this version does not know
# @kind: body
# @max: 80
agents-ask-other = користуватися частиною Znimok, якої ця версія не знає

# @where: Access question: allow this one call
# @kind: button
# @max: 20
agents-ask-once = Цього разу

# @where: Access question: allow until the agent's session ends
# @kind: button
# @max: 20
agents-ask-session = На цю сесію

# @where: Access question: allow from now on
# @kind: button
# @max: 20
agents-ask-always = Завжди

# @where: Access question: refuse
# @kind: button
# @max: 20
agents-ask-deny = Відмовити

# @where: Title of the question when an AI agent wants to delete a document for good; $name is the document
# @kind: title
# @max: 80
agents-confirm-delete-title = Видалити «{ $name }» назавжди?

# @where: Body of that question; $client is the name the agent reports
# @kind: body
# @max: 160
agents-confirm-delete-body = Про це просить ШІ-агент «{ $client }». Скасувати це буде неможливо.

# @where: Inside agents-ask-body after «wants to»: screen recording
# @kind: body
# @max: 80
agents-ask-record = записувати екран як відео, також коли вас немає поруч

# @where: Inside agents-ask-body after «wants to»: sound of a recording
# @kind: body
# @max: 80
agents-ask-record-audio = записувати звук комп'ютера й мікрофон

# @where: Settings → Agents: a permission in a client's list
# @kind: label
# @max: 24
agents-scope-record = запис екрана

# @where: Settings → Agents: a permission in a client's list
# @kind: label
# @max: 24
agents-scope-sound = звук і мікрофон

# @where: Report page: the button that brings the folded log back
# @kind: button
# @max: 12
report-log = Лог

# @where: Report page: tooltip of the buttons that fold the log pane or the details away
# @kind: tooltip
# @max: 20
report-fold = Згорнути

# @where: Access question: what «this session» and «always» cover
# @kind: body
# @max: 260
agents-ask-all = «На цю сесію» і «Завжди» дозволяють цьому агенту все: знімки, запис екрана, читання й зміну бібліотеки, налаштування — крім звуку запису, про який спитаємо окремо. «Цього разу» — лише цю дію.

# @where: Report page: the button that opens the baseline menu
# @kind: button
# @max: 14
report-cmp = Порівняти

# @where: Report page, baseline menu: keep this log in the browser under a name
# @kind: button
# @max: 30
report-cmp-save = Зберегти як еталон…

# @where: Report page: the question for the baseline's name
# @kind: body
# @max: 40
report-cmp-name = Ім'я еталона

# @where: Report page, baseline menu: a baseline file or another report's page
# @kind: button
# @max: 40
report-cmp-import = Імпортувати еталон чи звіт…

# @where: Report page, baseline menu: download this log's baseline as a file
# @kind: button
# @max: 30
report-cmp-export = Експортувати у файл

# @where: Report page: tooltip of the button that ends the comparison
# @kind: tooltip
# @max: 24
report-cmp-stop = Не порівнювати

# @where: Report page, baseline menu: nothing saved yet
# @kind: body
# @max: 40
report-cmp-none = Збережених еталонів ще немає

# @where: Report page: the line over the list while comparing; the variables are counts
# @kind: body
# @max: 100
report-cmp-sum = Еталон «{ name }»: +{ new } нових · −{ gone } зниклих · ↕{ moved } переставлених · ~{ changed } змінених

# @where: Report page: the chip that leaves only the differences in the list
# @kind: label
# @max: 20
report-cmp-only = Лише відмінності

# @where: Report page: tooltip of a struck-out row — the baseline had it, this recording does not
# @kind: tooltip
# @max: 40
report-cmp-gone = Є в еталоні, немає в цьому записі

# @where: Report page: tooltip of a row the baseline does not have
# @kind: tooltip
# @max: 30
report-cmp-new = Немає в еталоні

# @where: Report page: the browser's storage would not take the baseline
# @kind: body
# @max: 90
report-cmp-full = Сховище браузера переповнене — експортуйте еталон у файл.

# @where: Report page: the chip that filters PostHog's requests and console lines
# @kind: label
# @max: 12
report-chip-posthog = PostHog

# @where: Report page: the details tab of a PostHog request — its events, flags or replay packet
# @kind: label
# @max: 12
report-tab-posthog = PostHog

# @where: Report page: tooltip of the button that copies a request as a cURL command
# @kind: tooltip
# @max: 30
report-curl = Копіювати як cURL

# @where: Report page: tooltip of the button that copies a link to this event and moment
# @kind: tooltip
# @max: 40
report-link = Копіювати посилання на цей момент

# @where: Report page: tooltip of the search field — the operators it understands
# @kind: tooltip
# @max: 120
report-search-hint = Слова, -слова, status:5xx · method:post · host:posthog · type:xhr · dur>300

# @where: Report page, PostHog: how many feature flags came, how many are on
# @kind: body
# @max: 40
report-ph-flags = Прапорців: { n }, увімкнено { on }

# @where: Report page, PostHog: a session replay packet with so many snapshots
# @kind: body
# @max: 40
report-ph-replay = сесійний запис: { n } знімків

# @where: Report page: tooltip of a divider between panes
# @kind: tooltip
# @max: 60
report-drag = Потягніть, щоб змінити розмір

# @where: Report page: tooltip of the divider button that hides the log pane
# @kind: tooltip
# @max: 30
report-fold-log = Сховати лог

# @where: Report page: tooltip of the divider button that shows the hidden log pane
# @kind: tooltip
# @max: 30
report-show-log = Показати лог

# @where: Report page: tooltip of the divider button that hides the details pane
# @kind: tooltip
# @max: 30
report-fold-det = Сховати подробиці

# @where: Report page: tooltip of the divider button that shows the hidden details pane
# @kind: tooltip
# @max: 30
report-show-det = Показати подробиці

# @where: Report page, the facts under the video: label of the downloadable files (log.json, log.har…)
# @kind: label
# @max: 20
report-files = Файли

# @where: Report page: tooltip of a file link, before the file name
# @kind: tooltip
# @max: 20
report-download = Завантажити

# @where: Report page: tooltip of «⋯», the filters that do not fit
# @kind: tooltip
# @max: 30
report-more = Інші фільтри

# @where: Report page: tooltip of «Compare»
# @kind: tooltip
# @max: 80
report-cmp-hint = Зберегти лог як еталон або порівняти з еталоном

# @where: Report page: tooltip of «Copy» in the details
# @kind: tooltip
# @max: 60
report-copy-hint = Скопіювати те, що показують подробиці

# @where: Report page: tooltip of the strip of events under the video
# @kind: tooltip
# @max: 60
report-tl-hint = Клацніть або потягніть, щоб перемотати запис

## Extensions and integrations, sending to targets (ZK-101)


# @where: Settings: page in the left list — browser extension, Logi plugin, sharing targets
# @kind: label
# @max: 32
set-page-integrations = Розширення та інтеграції

# @where: Settings → Extensions and integrations: what the page is for
# @kind: body
# @max: 260
int-intro = Znimok у браузері й на пристроях Logitech, і куди «Надіслати в…» доставляє знімок, запис чи звіт. Токени й ключі лишаються в сховищі секретів системи.

# @where: Settings → Extensions and integrations: heading
# @kind: label
# @max: 30
int-extensions = Розширення

# @where: Settings → Extensions and integrations: the browser extension row
# @kind: label
# @max: 40
int-chrome = Розширення для Chrome і Edge

# @where: Settings → Extensions and integrations: status — the extension or plugin is connected now
# @kind: label
# @max: 20
int-connected = Підключено

# @where: Settings → Extensions and integrations: what the browser extension does (shown when not connected)
# @kind: body
# @max: 120
int-chrome-off = Лог DevTools браузера в записі і «Записати це вікно»

# @where: Settings → Extensions and integrations: button, open the Chrome Web Store page
# @kind: label
# @max: 24
int-chrome-store = Chrome Web Store

# @where: Settings → Extensions and integrations: the Logi Options+ plugin row
# @kind: label
# @max: 40
int-logi = Плагін для Logi Options+

# @where: Settings → Extensions and integrations: what the Logi plugin does (shown when not connected)
# @kind: body
# @max: 120
int-logi-off = Znimok на MX Creative Console та Actions Ring, вібровідгук на MX Master 4

# @where: Settings → Extensions and integrations: button, open the plugin's page
# @kind: label
# @max: 20
int-logi-page = Плагін

# @where: Settings → Extensions and integrations: heading of the sharing targets
# @kind: label
# @max: 30
int-targets = Надсилати в

# @where: Settings → Extensions and integrations: how the targets work
# @kind: body
# @max: 240
int-targets-hint = Увімкнена й заповнена ціль з'являється в «Надіслати в…» на картці після знімка і в листах експорту. Якщо до неї не достукатися, Znimok спробує пізніше.

# @where: Settings → Extensions and integrations: the target of a one-click «Send»
# @kind: label
# @max: 30
int-default = В один клік

# @where: Settings → Extensions and integrations: option — no default target, the menu asks
# @kind: label
# @max: 16
int-default-ask = Питати

# @where: Settings → Extensions and integrations: placeholder of a secret field that has a saved value
# @kind: label
# @max: 60
int-secret-saved = Збережено — введіть новий, щоб замінити

# @where: Settings → Extensions and integrations: placeholder, the Telegram bot token
# @kind: label
# @max: 30
int-tg-token = Токен бота

# @where: Settings → Extensions and integrations: placeholder, the Telegram chat
# @kind: label
# @max: 44
int-tg-chat = Чат за замовчуванням (необов'язково)

# @where: Settings → Extensions and integrations: button, find the chat that wrote to the bot
# @kind: label
# @max: 20
int-tg-find = Знайти чат

# @where: Settings → Extensions and integrations: no one has written to the bot yet
# @kind: body
# @max: 120
int-tg-no-chats = Чатів ще немає — напишіть боту будь-що в Telegram і спробуйте ще раз.

# @where: Settings → Extensions and integrations: button, check the connection
# @kind: label
# @max: 16
int-check = Перевірити

# @where: Settings → Extensions and integrations: a check runs
# @kind: label
# @max: 30
int-checking = Перевіряю…

# @where: Message sent to Telegram or a webhook by «Check»
# @kind: body
# @max: 80
int-probe = Znimok: з'єднання працює ✓

# @where: Settings → Extensions and integrations: placeholder, the Jira site
# @kind: label
# @max: 40
int-jira-site = Сайт: your-team.atlassian.net

# @where: Settings → Extensions and integrations: placeholder, the e-mail
# @kind: label
# @max: 16
int-jira-email = E-mail

# @where: Settings → Extensions and integrations: placeholder, the Jira API token
# @kind: label
# @max: 20
int-jira-token = API-токен

# @where: Settings → Extensions and integrations: placeholder, the project key
# @kind: label
# @max: 44
int-jira-project = Проєкт за замовчуванням (необов'язково)

# @where: Settings → Extensions and integrations: placeholder, an issue to attach to
# @kind: label
# @max: 24
int-jira-issue = Задача (необов'язково)

# @where: Settings → Extensions and integrations: placeholder, the type of a new issue
# @kind: label
# @max: 24
int-jira-type = Тип нової задачі

# @where: Settings → Extensions and integrations: placeholder, the Slack bot token
# @kind: label
# @max: 24
int-slack-token = Токен бота (xoxb-…)

# @where: Settings → Extensions and integrations: placeholder, the channel ID
# @kind: label
# @max: 44
int-slack-channel = Канал за замовчуванням (необов'язково)

# @where: Settings → Extensions and integrations: placeholder, the Redmine address
# @kind: label
# @max: 44
int-rm-url = Адреса: https://redmine.example.com

# @where: Settings → Extensions and integrations: placeholder, the Redmine API key
# @kind: label
# @max: 16
int-rm-key = API-ключ

# @where: Settings → Extensions and integrations: placeholder, the project identifier
# @kind: label
# @max: 44
int-rm-project = Проєкт за замовчуванням (необов'язково)

# @where: Settings → Extensions and integrations: placeholder, an issue number
# @kind: label
# @max: 32
int-rm-issue = Номер задачі (необов'язково)

# @where: Settings → Extensions and integrations: the webhooks heading
# @kind: label
# @max: 20
int-webhooks = Вебхуки

# @where: Settings → Extensions and integrations: what a webhook sends
# @kind: body
# @max: 240
int-webhooks-how = Для n8n, Zapier, Make чи власного сервера: POST із двома частинами, meta (JSON) і file; заданий вами заголовок, наприклад Authorization, іде разом.

# @where: Settings → Extensions and integrations: button, add a webhook
# @kind: label
# @max: 24
int-webhook-add = Додати вебхук

# @where: Settings → Extensions and integrations: a webhook without a name
# @kind: label
# @max: 16
int-webhook = Вебхук

# @where: Settings → Extensions and integrations: button, remove the webhook
# @kind: label
# @max: 16
int-webhook-remove = Прибрати

# @where: Settings → Extensions and integrations: placeholder, the webhook's name in the menu
# @kind: label
# @max: 24
int-webhook-name = Назва в меню

# @where: Settings → Extensions and integrations: placeholder, the header's name
# @kind: label
# @max: 32
int-webhook-header = Заголовок, напр. Authorization

# @where: Settings → Extensions and integrations: placeholder, the header's secret value
# @kind: label
# @max: 32
int-webhook-value = Його значення, напр. Bearer …

# @where: Toast: a file is on its way to a target; $target its name
# @kind: body
# @max: 80
share-queued = Надсилаю в { $target }…

# @where: Toast: a file was delivered; $target its name
# @kind: body
# @max: 80
share-sent = Надіслано в { $target }

# @where: Toast: sending failed for now, Znimok tries again later; $target its name
# @kind: body
# @max: 120
share-retrying = Не вдалося надіслати в { $target } — спробую ще

# @where: Toast: sending failed for good; $target its name
# @kind: body
# @max: 80
share-failed = Не надіслано в { $target }

# @where: Toast: the file could not be put into the sending queue
# @kind: body
# @max: 80
share-queue-error = Не вдалося підготувати надсилання

# @where: Export sheet: heading of the row of sharing targets
# @kind: label
# @max: 24
share-send-to = Надіслати в

# @where: After-capture card: button, send to the quick target; {0} its name
# @kind: label
# @max: 40
share-send-to-one = Надіслати в { $target }

# @where: Settings → Extensions and integrations: how Google Drive works
# @kind: body
# @max: 260
int-google-how = Увійдіть в акаунт Google у браузері. Znimok отримує доступ лише до файлів, які сам створює у вашому Drive (тека «Znimok»), і до адреси пошти — більше нічого у Drive чи пошті.

# @where: Settings → Extensions and integrations: button, sign out of this Google account
# @kind: label
# @max: 16
int-google-sign-out = Вийти

# @where: Settings → Extensions and integrations: switch, what is sent to Drive opens for anyone with the link
# @kind: label
# @max: 50
int-google-link = Відкрити за посиланням може будь-хто

# @where: Settings → Extensions and integrations: button, the first Google sign-in
# @kind: label
# @max: 30
int-google-sign-in = Увійти через Google

# @where: Settings → Extensions and integrations: button, sign in to one more Google account
# @kind: label
# @max: 30
int-google-add = Додати акаунт

# @where: Settings → Extensions and integrations: the browser shows Google's sign-in
# @kind: body
# @max: 60
int-google-waiting = Завершіть вхід у браузері…

# @where: Browser tab after the Google sign-in: title
# @kind: label
# @max: 50
int-google-done-title = Znimok увійшов у Google

# @where: Browser tab after the Google sign-in: text
# @kind: body
# @max: 80
int-google-done-text = Цю вкладку можна закрити й повернутися до Znimok.

# @where: Browser tab after a failed Google sign-in: title
# @kind: label
# @max: 50
int-google-failed-title = Znimok не ввійшов

# @where: Settings → Extensions and integrations: signed in, but Drive was not ticked on Google's page
# @kind: body
# @max: 120
int-google-no-drive = Google Drive не дозволено: увійдіть ще раз і позначте Google Drive

# @where: Toast: a file was delivered and its link is on the clipboard; $target its name
# @kind: body
# @max: 90
share-sent-link = Надіслано в { $target } — посилання скопійовано

# @where: Settings → Extensions and integrations: under several Google accounts
# @kind: body
# @max: 120
int-google-pick = Надсилання йде на позначений акаунт — клацніть інший, щоб перемкнутися.

# @where: Settings → Extensions and integrations: how to set up Telegram, step 1
# @kind: body
# @max: 200
int-tg-step1 = Відкрийте @BotFather у Telegram (кнопка нижче) і надішліть йому /newbot.

# @where: Settings → Extensions and integrations: how to set up Telegram, step 2
# @kind: body
# @max: 200
int-tg-step2 = Дайте ботові будь-яку назву, а потім ім'я користувача, що закінчується на «bot», напр. my_team_znimok_bot.

# @where: Settings → Extensions and integrations: how to set up Telegram, step 3
# @kind: body
# @max: 200
int-tg-step3 = BotFather відповість токеном — довгим рядком на кшталт 123456789:AAH…. Скопіюйте його й вставте в поле нижче.

# @where: Settings → Extensions and integrations: how to set up Telegram, step 4
# @kind: body
# @max: 200
int-tg-step4 = Відкрийте свого нового бота й натисніть «Почати» (Start). Для групи чи каналу: додайте туди бота й напишіть будь-що.

# @where: Settings → Extensions and integrations: how to set up Telegram, step 5
# @kind: body
# @max: 200
int-tg-step5 = Натисніть «Знайти чат» — Znimok знайде його сам.

# @where: Settings → Extensions and integrations: button, opens the page needed to set up Telegram
# @kind: label
# @max: 36
int-tg-open = Відкрити @BotFather

# @where: Settings → Extensions and integrations: how to set up Jira, step 1
# @kind: body
# @max: 200
int-jira-step1 = Сайт — адреса, за якою ви відкриваєте Jira, напр. your-team.atlassian.net.

# @where: Settings → Extensions and integrations: how to set up Jira, step 2
# @kind: body
# @max: 200
int-jira-step2 = Пошта — та, з якою ви входите в Jira.

# @where: Settings → Extensions and integrations: how to set up Jira, step 3
# @kind: body
# @max: 200
int-jira-step3 = Відкрийте сторінку API-токенів (кнопка нижче), натисніть «Create API token», назвіть його Znimok, скопіюйте токен і вставте нижче.

# @where: Settings → Extensions and integrations: how to set up Jira, step 4
# @kind: body
# @max: 200
int-jira-step4 = Ключ проєкту — літери перед номером задачі: ZK у ZK-101. Задача необов'язкова — без неї кожне надсилання створює нову.

# @where: Settings → Extensions and integrations: how to set up Jira, step 5
# @kind: body
# @max: 200
int-jira-step5 = Натисніть «Перевірити».

# @where: Settings → Extensions and integrations: button, opens the page needed to set up Jira
# @kind: label
# @max: 36
int-jira-open = Відкрити сторінку API-токенів

# @where: Settings → Extensions and integrations: how to set up Slack, step 1
# @kind: body
# @max: 200
int-slack-step1 = Натисніть «Створити застосунок Slack» нижче: Slack відкриється з усім уже заповненим. Виберіть свій робочий простір, далі Next і Create.

# @where: Settings → Extensions and integrations: how to set up Slack, step 2
# @kind: body
# @max: 200
int-slack-step2 = На сторінці застосунку натисніть «Install to Workspace», потім «Allow».

# @where: Settings → Extensions and integrations: how to set up Slack, step 3
# @kind: body
# @max: 200
int-slack-step3 = Відкрийте «OAuth & Permissions», скопіюйте «Bot User OAuth Token» (xoxb-…) і вставте нижче.

# @where: Settings → Extensions and integrations: how to set up Slack, step 4
# @kind: body
# @max: 200
int-slack-step4 = У потрібному каналі Slack напишіть /invite @Znimok, щоб застосунок міг туди писати.

# @where: Settings → Extensions and integrations: how to set up Slack, step 5
# @kind: body
# @max: 200
int-slack-step5 = ID каналу: натисніть на назву каналу — він у самому низу вікна (C0…). Скопіюйте, вставте нижче й натисніть «Перевірити».

# @where: Settings → Extensions and integrations: button, opens the page needed to set up Slack
# @kind: label
# @max: 36
int-slack-open = Створити застосунок Slack

# @where: Settings → Extensions and integrations: how to set up Redmine, step 1
# @kind: body
# @max: 200
int-rm-step1 = Адреса — те, за чим ви відкриваєте Redmine, напр. https://redmine.example.com — впишіть її першою.

# @where: Settings → Extensions and integrations: how to set up Redmine, step 2
# @kind: body
# @max: 200
int-rm-step2 = Натисніть «Відкрити Мій обліковий запис» нижче: праворуч знайдіть «Ключ доступу до API» → «Показати», скопіюйте ключ і вставте нижче.

# @where: Settings → Extensions and integrations: how to set up Redmine, step 3
# @kind: body
# @max: 200
int-rm-step3 = Такого розділу немає? Попросіть адміністратора Redmine увімкнути REST API (Адміністрування → Налаштування → API).

# @where: Settings → Extensions and integrations: how to set up Redmine, step 4
# @kind: body
# @max: 200
int-rm-step4 = Ідентифікатор проєкту — в його адресі: …/projects/ідентифікатор. Номер задачі необов'язковий. Потім «Перевірити».

# @where: Settings → Extensions and integrations: button, opens the page needed to set up Redmine
# @kind: label
# @max: 36
int-rm-open = Відкрити Мій обліковий запис

# @where: Settings → Extensions and integrations: button, show the steps of setting up a target again
# @kind: label
# @max: 30
int-help-show = Як налаштувати

# @where: Settings → Extensions and integrations: button, hide the steps of setting up a target
# @kind: label
# @max: 30
int-help-hide = Сховати кроки

# @where: Editor's top bar: button next to «Copy», sends to a connected service
# @kind: label
# @max: 14
doc-share = Поширити

# @where: Editor: «Share» menu when nothing is connected yet
# @kind: body
# @max: 140
doc-share-none = Підключіть Google Drive, Telegram, Slack чи Jira один раз — і надсилайте сюди в один клік.

# @where: Editor: «Share» menu, opens Settings → Extensions and integrations
# @kind: label
# @max: 40
doc-share-connect = Підключити…

# @where: Settings → Extensions and integrations: a webhook, what «Share» sends to it
# @kind: label
# @max: 30
int-webhook-what = Що надсилати

# @where: Settings → Extensions and integrations: a webhook, sends the video (MP4) or the screenshot (PNG)
# @kind: label
# @max: 24
int-webhook-media = Відео чи знімок

# @where: Settings → Extensions and integrations: a webhook, sends the whole file: a recording's report package (.zreport), a screenshot's document (.znimok)
# @kind: label
# @max: 24
int-webhook-all = Увесь файл

# @where: Settings → Extensions and integrations: a webhook, sends only a recording's logs (JSON); a screenshot goes as a picture
# @kind: label
# @max: 24
int-webhook-logs = Лише логи

# @where: Settings → Extensions and integrations: a webhook: what the three choices mean
# @kind: body
# @max: 220
int-webhook-what-hint = Увесь файл: запис — пакетом звіту (відео, логи, подробиці), знімок — документом Znimok. Лише логи: консоль браузера, мережа й dataLayer як JSON (знімок іде картинкою).

# @where: The «Share» window: the list of connected services
# @kind: label
# @max: 20
share-where = Куди

# @where: The «Share» window: the channel, project, issue or chat inside the service
# @kind: label
# @max: 24
share-place = Куди саме

# @where: The «Share» window: placeholder of the field that searches the places or takes one typed
# @kind: label
# @max: 60
share-place-search = Пошук або вписати: канал, проєкт, задача, чат

# @where: The «Share» window: a row that sends to what was typed; $place the typed text
# @kind: label
# @max: 50
share-place-use = Використати «{ $place }»

# @where: The «Share» window: the service's list is being fetched
# @kind: label
# @max: 40
share-places-loading = Завантажую список…

# @where: The «Share» window: nothing chosen where exactly
# @kind: body
# @max: 60
share-place-needed = Виберіть або впишіть, куди саме

# @where: The «Share» window: what is sent
# @kind: label
# @max: 20
share-what = Що

# @where: The «Share» window: a screenshot as a picture (PNG)
# @kind: label
# @max: 22
share-what-image = Картинка

# @where: The «Share» window: the Znimok document itself
# @kind: label
# @max: 22
share-what-document = Документ Znimok

# @where: The «Share» window: a recording as a video (MP4)
# @kind: label
# @max: 22
share-what-video = Відео

# @where: The «Share» window: a recording as a page with the video and its log
# @kind: label
# @max: 26
share-what-report = Відео з логом (HTML)

# @where: The «Share» window: only a recording's logs (JSON)
# @kind: label
# @max: 22
share-what-logs = Лише логи

# @where: The «Share» window: switch, hide the log's sensitive values
# @kind: label
# @max: 40
share-hide = Приховати чутливе

# @where: The «Share» window: placeholder of the comment that goes with the file
# @kind: label
# @max: 40
share-comment = Коментар (необов'язково)

# @where: The «Share» window: button, sends
# @kind: label
# @max: 16
share-send = Надіслати

# @where: Settings → Extensions and integrations: the label at the top, the integrations are experimental
# @kind: label
# @max: 30
int-experimental-badge = Експериментальна функція

# @where: Settings → Extensions and integrations: the note at the top, the integrations are experimental
# @kind: body
# @max: 160
int-experimental = Інтеграції ще в роботі — щось може змінитися або поки не працювати. Розкажіть, що пішло не так.

# @where: Settings → Extensions and integrations: button, one more account of the service (another workspace, site, bot)
# @kind: label
# @max: 30
int-account-add = Додати обліковий запис

# @where: Settings → Extensions and integrations: placeholder, the account's name shown in «Share»
# @kind: label
# @max: 44
int-account-name = Назва в «Поширити», напр. «Клієнт А»

# @where: Settings → Extensions and integrations: button, removes this account and its token
# @kind: label
# @max: 16
int-account-remove = Прибрати

# @where: Toast: the file is in Drive and Gmail's new letter opened with its link; $target the account
# @kind: body
# @max: 90
share-gmail-opened = Лист відкрито в Gmail — допишіть, кому

# @where: Settings → Agents: a permission in a client's list
# @kind: label
# @max: 24
agents-scope-share = надсилання назовні

# @where: The permission question: what an agent asks for — sending to the connected services
# @kind: body
# @max: 120
agents-ask-share = надсилати документи у ваші підключені сервіси (Google Drive, Gmail, Telegram, Jira, Slack, Redmine, вебхуки)

# @where: Toast: an agent sent a document; $client the agent, $target where
# @kind: body
# @max: 90
share-agent-queued = { $client } надсилає в { $target }…

# @where: Settings → Extensions and integrations: button, sign in to a Slack workspace in the browser
# @kind: label
# @max: 30
int-slack-sign-in = Увійти в Slack

# @where: Settings → Extensions and integrations: how signing in to Slack works
# @kind: body
# @max: 200
int-slack-sign-in-hint = Найпростіше — увійти в Slack у браузері: Znimok надсилатиме від вашого імені в канали, які ви оберете під час поширення. Жодних токенів шукати не треба.

# @where: Settings → Extensions and integrations: an account signed in through the browser; $name the workspace or site
# @kind: label
# @max: 60
int-signed-in = Вхід виконано: { $name }

# @where: Browser tab after signing in to a service: title
# @kind: label
# @max: 50
int-sign-in-done-title = Znimok увійшов

# @where: Settings → Extensions and integrations: button, sign in to Atlassian (Jira) in the browser
# @kind: label
# @max: 30
int-jira-sign-in = Увійти в Atlassian

# @where: Settings → Extensions and integrations: how signing in to Jira works
# @kind: body
# @max: 200
int-jira-sign-in-hint = Найпростіше — увійти в Atlassian у браузері: API-токен створювати не треба. Якщо сайтів кілька, спершу впишіть сайт — Znimok візьме саме його.

# @where: The «Share» window: the place chosen, shown above the list; $name its name
# @kind: label
# @max: 60
share-place-chosen = Вибрано: { $name }

# @where: The «Share» window: button ✕, clears the place chosen
# @kind: label
# @max: 24
share-place-clear = Зняти вибір

# @where: Updates page: the reason when the updater gave no answer to a check
# @kind: status
# @max: 60
upd-no-answer = оновлювач не відповів — спробуйте ще раз
