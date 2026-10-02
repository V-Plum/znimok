### Znimok — interface strings, English (en).
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
app-tagline = Screenshots and screen recordings with annotations

## Common
## Words shared by many screens. Use these ids instead of copying the same text.


# @where: Dialogs and modes: leave without applying
# @kind: button
# @max: 16
common-cancel = Cancel

# @where: Icon-only close buttons (×) of dialogs, pills, tabs
# @kind: a11y
# @max: 24
common-close = Close

# @where: Crop mode, overlay editor: finish and apply
# @kind: button
# @max: 16
common-done = Done

# @where: Assistant plan, dialogs: apply the change
# @kind: button
# @max: 16
common-apply = Apply

# @where: Crop, tone and similar: back to the original value
# @kind: button
# @max: 16
common-reset = Reset

# @where: Option: size taken from the content (text block width)
# @kind: option
# @max: 8
common-auto = Auto

# @where: Save to the library (Ctrl+S); also confirm-on-close dialog
# @kind: button
# @max: 16
common-save = Save

# @where: Export dialogs: opens the system save dialog
# @kind: button
# @max: 18
common-save-ellipsis = Save…

# @where: Close-with-unsaved dialog, the quiet destructive choice
# @kind: button
# @max: 18
common-dont-save = Don't save

# @where: Context menus: delete the selected item
# @kind: menu
# @max: 20
common-delete = Delete

# @where: Context menus: rename (F2)
# @kind: menu
# @max: 20
common-rename = Rename

# @where: Context menus: duplicate (Ctrl+D)
# @kind: menu
# @max: 20
common-duplicate = Duplicate

# @where: Title bar menu button, file lists
# @kind: button
# @max: 16
common-open = Open

# @where: Library home: open a file from disk
# @kind: button
# @max: 18
common-open-ellipsis = Open…

# @where: Toasts: reveal the result (file, item)
# @kind: button
# @max: 16
common-show = Show

# @where: Context menu of a mark: exclude from export (keeps it in the document)
# @kind: menu
# @max: 20
common-hide = Hide

# @where: Icon-only "…" buttons that open more actions
# @kind: a11y
# @max: 24
common-more = More

# @where: Onboarding, banners: postpone
# @kind: button
# @max: 16
common-later = Later

# @where: Onboarding: next step
# @kind: button
# @max: 16
common-next = Next

# @where: Settings and onboarding: choose another folder
# @kind: button
# @max: 16
common-change-ellipsis = Change…

# @where: Inspector: add fill / effect / tag
# @kind: button
# @max: 16
common-add = Add

# @where: Settings: add a share target, a key
# @kind: button
# @max: 16
common-add-ellipsis = Add…

# @where: Settings row action that opens the page with details
# @kind: button
# @max: 16
common-manage = Manage

# @where: Errors about permissions: re-test after the user changed settings
# @kind: button
# @max: 20
common-check-again = Check again

# @where: Segmented controls and pickers: nothing selected (effect, arrowhead, outline)
# @kind: option
# @max: 12
common-none = None

# @where: Tooltip of any control that is shown but not implemented yet (stable layout rule)
# @kind: tooltip
# @max: 40
# @note: Shown on greyed-out controls so their place does not change between releases.
common-in-development = Coming soon

# @where: Three-way segmented control Never / Ask / Always
# @kind: option
# @max: 10
common-never = Never

# @where: Three-way segmented control Never / Ask / Always
# @kind: option
# @max: 10
common-ask = Ask

# @where: Three-way segmented control Never / Ask / Always
# @kind: option
# @max: 10
common-always = Always

# @where: Short value "switched off" in segmented controls
# @kind: option
# @max: 10
common-off = Off

# @where: Relative time in logs: just now
# @kind: status
# @max: 14
common-now = now

# @where: Relative time in logs and library groups
# @kind: status
# @max: 14
common-yesterday = yesterday

# @where: Library: group header of today's items
# @kind: heading
# @max: 14
common-today = Today

# @where: Library: group header of yesterday's items
# @kind: heading
# @max: 14
common-yesterday-heading = Yesterday

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
        [one] { $count } annotation
       *[other] { $count } annotations
    }

## Navigation
## The left rail of the main window and the way back to the library.


# @where: Left rail: Library page (also its accessible name)
# @kind: tooltip
# @max: 24
nav-library = Library

# @where: Left rail: Agents page
# @kind: tooltip
# @max: 24
nav-agents = Agents

# @where: Left rail: Settings page
# @kind: tooltip
# @max: 24
nav-settings = Settings

# @where: Editor title bar: back arrow to the library
# @kind: a11y
# @max: 32
nav-back-to-library = Back to library

## Editor title bar
## The row at the top of the editor window: document kind, name, save state, main actions.


# @where: Title bar chip before the document name
# @kind: badge
# @max: 12
doc-kind-screenshot = Screenshot

# @where: Title bar chip before the document name
# @kind: badge
# @max: 12
doc-kind-video = Video

# @where: Title bar chip for an empty canvas
# @kind: badge
# @max: 16
doc-kind-blank = Blank canvas

# @where: Title bar toggle; global setting, on by default
# @kind: label
# @max: 24
doc-autosave = Save automatically

# @where: Title bar, after the toggle: everything is in the library
# @kind: status
# @max: 16
doc-state-saved = saved

# @where: Title bar: there are changes not in the library yet (auto-save off)
# @kind: status
# @max: 16
doc-state-unsaved = unsaved

# @where: Title bar: saving right now
# @kind: status
# @max: 16
doc-state-saving = saving…

# @where: Title bar undo button; shortcut shown by the tooltip system
# @kind: tooltip
# @max: 28
doc-undo = Undo

# @where: Title bar redo button
# @kind: tooltip
# @max: 28
doc-redo = Redo

# @where: Title bar: give the document to an AI agent (file + context)
# @kind: button
# @max: 20
doc-hand-to-agent = Hand to agent

# @where: Title bar main button: copy the image to the clipboard
# @kind: button
# @max: 16
# @note: Main action everywhere. Never call it "Export".
doc-copy = Copy

# @where: Arrow next to Copy: other ways to get the result out
# @kind: tooltip
# @max: 24
doc-other-ways = More options

# @where: Name of a new document before it is saved; $date and $time are preformatted
# @kind: label
# @max: 40
doc-untitled = Screenshot { $date } { $time }

# @where: Question: the document clicked in the library is open in another window
# @kind: heading
# @max: 80
open-twice-title = «{ $name }» is open in another window

# @where: Question body: why a copy, not a second window on the same file
# @kind: body
# @max: 200
open-twice-body = Two windows saving one document would overwrite each other's changes. Go to that window, or open a copy — it is saved as a new document.

# @where: Button: bring the window with the document to the front
# @kind: button
# @max: 24
open-twice-go = Go to the window

# @where: Button: open a copy that saves as a new document
# @kind: button
# @max: 24
open-twice-copy = Open a copy

# @where: The name of a copy of a document
# @kind: label
# @max: 80
doc-copy-name = { $name } (copy)

## Open menu
## The "Open ▾" menu in the title bar and on the home screen.


# @where: Open ▾ menu; shortcut Ctrl+O / ⌘O shown by the menu
# @kind: menu
# @max: 32
open-file = File…

# @where: Open ▾ menu: image from the clipboard
# @kind: menu
# @max: 32
open-clipboard = From clipboard

# @where: Open ▾ menu: submenu with capture kinds
# @kind: menu
# @max: 32
open-new-shot = New screenshot

# @where: Open ▾ menu: empty canvas
# @kind: menu
# @max: 32
open-blank = Blank canvas

# @where: Toast when "From clipboard" finds no image
# @kind: error
# @max: 80
open-error-no-image = There is no image in the clipboard.

# @where: Toast when a dropped or opened file cannot be read as an image
# @kind: error
# @max: 120
open-error-not-image = Can't open “{ $name }”: it is not an image Znimok can read.

## Other ways menu
## The menu behind the arrow next to Copy.


# @where: First item, same as the main Copy button
# @kind: menu
# @max: 32
share-copy-image = Copy image

# @where: Tooltip tail of the Copy button: it can be dragged out as a file
# @kind: tooltip
# @max: 32
share-drag-tip = drag to a chat or a folder

# @where: Grey note after the first item
# @kind: hint
# @max: 24
share-copy-image-hint = main button

# @where: Puts the file itself on the clipboard (paste into a chat or a folder)
# @kind: menu
# @max: 32
share-copy-file = Copy as file

# @where: Opens the OS share sheet
# @kind: menu
# @max: 36
share-system = Share…

# @where: Same as share-system where the menu names the OS feature
# @kind: menu
# @max: 40
share-system-long = System share menu…

# @where: Opens the export dialog (Ctrl+Shift+S)
# @kind: menu
# @max: 32
share-export = Export file…

# @where: Other ways menu: a copy of the document (.znimok) in any folder
# @kind: menu
# @max: 24
doc-save-as = Save as…

# @where: Status line after «Save as…»
# @kind: toast
# @max: 48
doc-saved-as = Saved “{ $name }”

# @where: Single HTML file with marks that opens anywhere
# @kind: menu
# @max: 36
share-html = Self-contained HTML…

# @where: Group heading for integrations
# @kind: heading
# @max: 24
share-targets = Share targets

# @where: Badge next to the heading: not available yet
# @kind: badge
# @max: 12
share-targets-later = later

# @where: Adds an integration
# @kind: menu
# @max: 24
share-add-target = Add target…

## Tools
## The vertical tool rail of the editor and the overlay. Tooltips carry the one-key shortcut in parentheses; keep the Latin letter as is — it is a physical key, not a word. Names without the key are used as headings.


# @where: Tool rail button tooltip and accessible name; (V) is its shortcut
# @kind: tooltip
# @max: 28
tool-select = Select (V)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-select-name = Select

# @where: Tool rail button tooltip and accessible name; (R) is its shortcut
# @kind: tooltip
# @max: 28
tool-rect = Rectangle (R)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-rect-name = Rectangle

# @where: Tool rail button tooltip and accessible name; (E) is its shortcut
# @kind: tooltip
# @max: 28
tool-ellipse = Ellipse (E)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-ellipse-name = Ellipse

# @where: Tool rail button tooltip and accessible name; (L) is its shortcut
# @kind: tooltip
# @max: 28
tool-arrow = Arrow (L)

# @where: Tool rail button tooltip and accessible name; (L) is its shortcut. Heads are properties of the line
# @kind: tooltip
# @max: 28
tool-line = Line (L)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-arrow-name = Arrow

# @where: Tool rail button tooltip and accessible name; (P) is its shortcut
# @kind: tooltip
# @max: 28
tool-pen = Pen (P)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-pen-name = Pen

# @where: Tool rail button tooltip and accessible name; (T) is its shortcut
# @kind: tooltip
# @max: 28
tool-text = Text (T)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-text-name = Text

# @where: Tool rail button tooltip and accessible name; (B) is its shortcut
# @kind: tooltip
# @max: 28
tool-hide = Hide (B)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-hide-name = Hide

# @where: Tool rail button tooltip and accessible name; (H) is its shortcut
# @kind: tooltip
# @max: 28
tool-highlighter = Highlighter (H)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-highlighter-name = Highlighter

# @where: Tool rail button tooltip and accessible name; (N) is its shortcut
# @kind: tooltip
# @max: 28
tool-counter = Counter (N)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-counter-name = Counter

# @where: Inspector title of a selected counter: its numbering group
# @kind: heading
# @max: 32
counter-group-title = Counter — Group { $n }

# @where: Inspector, Counter: button that selects the whole numbering group (tooltip says more)
# @kind: button
# @max: 24
counter-edit-group-short = Edit the whole group

# @where: Tool rail button tooltip and accessible name; (S) is its shortcut
# @kind: tooltip
# @max: 28
tool-stamp = Stamp (S)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-stamp-name = Stamp

# @where: Tool rail button tooltip and accessible name; (I) is its shortcut
# @kind: tooltip
# @max: 28
tool-image = Image (I)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-image-name = Image

# @where: Tooltip of the Image button, after its name
# @kind: tooltip
# @max: 96
tool-image-tip = Put a picture from a file on the screenshot (Ctrl+V pastes one from the clipboard)

# @where: Tool rail button tooltip and accessible name; (C) is its shortcut
# @kind: tooltip
# @max: 28
tool-crop = Crop (C)

# @where: Tool / mark kind name without the shortcut: inspector heading, layer rows
# @kind: label
# @max: 20
tool-crop-name = Crop

# @where: Mark kind name: a straight line (arrow with no heads)
# @kind: label
# @max: 20
tool-line-name = Line

# @where: Video editor rail: split the clip at the playhead; (S) is its shortcut
# @kind: tooltip
# @max: 28
tool-cut = Split (S)

# @where: Narrow windows: the rest of the tools behind "…"
# @kind: tooltip
# @max: 24
tool-more = More tools

## Context bar
## The floating bar next to the selected mark (window, overlay and video share it).


# @where: Colour swatch button for outlines
# @kind: a11y
# @max: 32
ctx-stroke-colour = Stroke colour

# @where: Colour swatch button for fills, with a colour set
# @kind: a11y
# @max: 32
ctx-fill-colour = Fill colour

# @where: Colour swatch button for fills when there is no fill
# @kind: a11y
# @max: 32
ctx-fill-none = Fill colour: none

# @where: Text mark: colour of the letters
# @kind: a11y
# @max: 32
ctx-text-colour = Text colour

# @where: Text mark: colour of the outline around the letters
# @kind: a11y
# @max: 32
ctx-text-outline = Outline colour

# @where: Tooltip of the "none" swatch in a text's outline colours
# @kind: tooltip
# @max: 32
ctx-outline-none = No outline

# @where: Line width option
# @kind: a11y
# @max: 16
ctx-thin = Thin

# @where: Line width option
# @kind: a11y
# @max: 16
ctx-medium = Medium

# @where: Line width option
# @kind: a11y
# @max: 16
ctx-thick = Thick

# @where: Corner rounding button
# @kind: a11y
# @max: 16
ctx-corners = Corners

# @where: Solid / dashed button
# @kind: a11y
# @max: 24
ctx-line-style = Line style

# @where: Shadow / glow button
# @kind: a11y
# @max: 16
ctx-effect = Effect

# @where: Text mark
# @kind: a11y
# @max: 16
ctx-bold = Bold

# @where: Text mark
# @kind: a11y
# @max: 16
ctx-italic = Italic

# @where: Text alignment
# @kind: a11y
# @max: 24
ctx-align-left = Align left

# @where: Text alignment
# @kind: a11y
# @max: 24
ctx-align-centre = Align centre

# @where: Text alignment
# @kind: a11y
# @max: 24
ctx-align-right = Align right

# @where: Status hint while drawing a rectangle/ellipse
# @kind: hint
# @max: 32
# @note: "Shift" is a key name, keep it.
ctx-hint-square = Shift — square

# @where: Status hint on the canvas
# @kind: hint
# @max: 32
ctx-hint-zoom = Alt + wheel — zoom

## Colour picker
## The popover that opens from a colour swatch.


# @where: Accessible name of the popover
# @kind: a11y
# @max: 24
colour-picker = Colour picker

# @where: Button that picks a colour from the screenshot or the screen
# @kind: button
# @max: 16
colour-eyedropper = Eyedropper

# @where: Tooltip of the eyedropper button
# @kind: tooltip
# @max: 72
colour-eyedropper-tip = Eyedropper: pick a colour from the screenshot or the screen

# @where: Label of the hex code field; keep "HEX"
# @kind: label
# @max: 6
colour-hex = HEX

# @where: Opacity field (0–100 %)
# @kind: a11y
# @max: 16
colour-opacity = Opacity

# @where: Row of recently used colours
# @kind: heading
# @max: 16
colour-recent = Recent

# @where: Adds the colour to the user's palette
# @kind: button
# @max: 24
colour-save = Save to palette

# @where: Grey note at the bottom of the popover
# @kind: hint
# @max: 160
colour-eyedropper-note = The eyedropper samples the screenshot; for the live screen it uses capture (Windows) or the system sampler (macOS).

# @where: Toast while the eyedropper is armed: the next click on the picture picks its colour
# @kind: hint
# @max: 80
colour-eyedropper-hint = Click the picture to take its colour · Esc cancels

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-red = Red

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-orange = Orange

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-yellow = Yellow

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-green = Green

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-blue = Blue

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-purple = Purple

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-white = White

# @where: Palette swatch (accessible name only)
# @kind: a11y
# @max: 16
colour-black = Black

## Inspector
## The right panel of the editor window: tabs and the Object tab.


# @where: Inspector tab: the selected mark
# @kind: tab
# @max: 10
insp-tab-object = Object

# @where: Inspector tab: list of marks, z-order
# @kind: tab
# @max: 10
insp-tab-layers = Layers

# @where: Inspector tab: the screenshot itself (crop, rotation, tone)
# @kind: tab
# @max: 10
insp-tab-shot = Image

# @where: Inspector tab: title, author, tags…
# @kind: tab
# @max: 10
insp-tab-meta = Meta

# @where: Video inspector tab: trim and sound
# @kind: tab
# @max: 10
insp-tab-clip = Clip

# @where: Video inspector tab: DevTools log and clicks
# @kind: tab
# @max: 10
insp-tab-events = Events

# @where: Collapse the inspector
# @kind: a11y
# @max: 24
insp-hide-panel = Hide panel

# @where: Expand the inspector
# @kind: a11y
# @max: 24
insp-show-panel = Show panel

# @where: After the kind name in the Object tab: which mark of how many
# @kind: label
# @max: 24
insp-selected-of = { $index } of { $total }

# @where: Object tab when nothing is selected
# @kind: hint
# @max: 80
insp-nothing-selected = Select an annotation to change it, or pick a tool on the left.

# @where: Object tab of a screenshot without marks
# @kind: hint
# @max: 100
insp-no-marks = No annotations yet. Pick a tool on the left and draw on the screenshot.

# @where: Section: outline colour and width
# @kind: label
# @max: 16
insp-stroke = Stroke

# @where: Section: fill colour
# @kind: label
# @max: 16
insp-fill = Fill

# @where: Object panel: row label of the line thickness control
# @kind: label
# @max: 12
insp-thickness = Thickness

# @where: Object panel: the "no outline" chip in the stroke colour row (rectangle, ellipse)
# @kind: tooltip
# @max: 24
insp-stroke-none = No outline

# @where: Object panel: section with X / Y / W / H of the selected annotation
# @kind: label
# @max: 24
insp-position = Position and size

# @where: Object panel: section with X / Y of an annotation whose size is not typed (text, pen, marker, counter, stamp)
# @kind: label
# @max: 24
insp-place = Position

# @where: Inspector, Marker: tooltip of the fourth (thickest) thickness button
# @kind: tooltip
# @max: 24
marker-extra-thick = Extra thick

# @where: Fill section: button that swaps the stroke and fill colours (text: letters and outline)
# @kind: tooltip
# @max: 40
insp-swap-colours = Swap stroke and fill

# @where: Section: corner rounding
# @kind: label
# @max: 16
insp-corners = Corners

# @where: Corner option
# @kind: option
# @max: 10
insp-corners-sharp = Sharp

# @where: Corner option
# @kind: option
# @max: 10
insp-corners-soft = Soft

# @where: Corner option
# @kind: option
# @max: 10
insp-corners-round = Round

# @where: Section: opacity slider
# @kind: label
# @max: 16
insp-opacity = Opacity

# @where: Section: list of effects
# @kind: label
# @max: 16
insp-effects = Effects

# @where: Effects section when empty
# @kind: hint
# @max: 24
insp-effects-none = No effects

# @where: Effect name (list row and Add menu)
# @kind: menu
# @max: 16
insp-effect-shadow = Shadow

# @where: Effect name
# @kind: menu
# @max: 16
insp-effect-glow = Glow

# @where: Effect name
# @kind: menu
# @max: 16
insp-effect-outline = Outline

# @where: Toggle of an effect row; $name is the effect name
# @kind: a11y
# @max: 32
insp-effect-on = { $name } on

# @where: Toggle of an effect row
# @kind: a11y
# @max: 32
insp-effect-off = { $name } off

# @where: Chevron that opens an effect's settings
# @kind: a11y
# @max: 16
insp-expand = Expand

# @where: Chevron that closes an effect's settings
# @kind: a11y
# @max: 16
insp-collapse = Collapse

# @where: Shadow setting (short label before a number field)
# @kind: label
# @max: 10
insp-shadow-offset = Offset

# @where: Shadow setting (short label)
# @kind: label
# @max: 10
# @note: Very short label; the field's accessible name is insp-shadow-blur-long.
insp-shadow-blur = Blur

# @where: Accessible name of the shadow blur field
# @kind: a11y
# @max: 24
insp-shadow-blur-long = Blur radius

# @where: Accessible name of the shadow opacity field
# @kind: a11y
# @max: 24
insp-shadow-opacity = Shadow opacity

# @where: Design note for developers is not shown; this is the hint under the list
# @kind: hint
# @max: 160
insp-effects-note = Effects stack: add, switch on and off, reorder.

# @where: Effect strength option: weak
# @kind: option
# @max: 12
insp-effect-light = Light

# @where: Effect strength option: strong
# @kind: option
# @max: 12
insp-effect-strong = Strong

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
insp-line-points = Line ends

# @where: Width field label, one letter
# @kind: label
# @max: 2
insp-w = W

# @where: Height field label, one letter
# @kind: label
# @max: 2
insp-h = H

# @where: Accessible name of the width field
# @kind: a11y
# @max: 16
insp-width = Width

# @where: Accessible name of the height field
# @kind: a11y
# @max: 16
insp-height = Height

# @where: Rotation field (degrees)
# @kind: label
# @max: 16
insp-rotation = Rotation

# @where: Line style option
# @kind: a11y
# @max: 20
insp-line-solid = Solid

# @where: Line style option
# @kind: a11y
# @max: 20
insp-line-dashed = Dashed

# @where: Line style option
# @kind: a11y
# @max: 20
insp-line-dashdot = Dash-dot

# @where: Arrow / line / pen section: arrowheads
# @kind: label
# @max: 20
insp-heads = Arrowheads

# @where: Grey note after the heading: heads also work for the pen
# @kind: hint
# @max: 24
insp-heads-pen = pen too

# @where: Arrowhead at the start of the line
# @kind: label
# @max: 10
insp-head-start = Start

# @where: Arrowhead at the end of the line
# @kind: label
# @max: 10
insp-head-end = End

# @where: Arrowhead size (S / M / L)
# @kind: label
# @max: 10
insp-head-size = Size

# @where: Arrowhead shape
# @kind: a11y
# @max: 16
insp-head-triangle = Triangle

# @where: Arrowhead shape: open V
# @kind: a11y
# @max: 16
insp-head-chevron = Chevron

# @where: Arrowhead shape
# @kind: a11y
# @max: 16
insp-head-dot = Dot

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
text-editing = editing

# @where: Section: typeface
# @kind: label
# @max: 12
text-font = Font

# @where: Font size field
# @kind: label
# @max: 12
text-size = Size

# @where: Section: text block width
# @kind: label
# @max: 12
text-block = Block

# @where: Accessible name of the block width field
# @kind: a11y
# @max: 24
text-block-width = Block width

# @where: Under the block width
# @kind: hint
# @max: 100
text-block-hint = Width 0 — no wrapping; the side handles change the width.

# @where: Section: outline around letters
# @kind: label
# @max: 12
text-outline = Outline

# @where: Status hint: Enter finishes
# @kind: hint
# @max: 16
text-hint-done = done

# @where: Status hint: Shift+Enter adds a line
# @kind: hint
# @max: 16
text-hint-newline = new line

# @where: Status hint: Esc cancels
# @kind: hint
# @max: 16
text-hint-cancel = cancel

# @where: Status bar while a text mark is edited
# @kind: status
# @max: 24
text-status = editing text

# @where: Button: smaller type
# @kind: tooltip
# @max: 24
text-size-down = Smaller type

# @where: Button: larger type
# @kind: tooltip
# @max: 24
text-size-up = Larger type

## Hide, highlighter, counter, stamp
## Mark-specific controls.


# @where: Hide style: blur what is under the mark
# @kind: option
# @max: 12
hide-blur = Blur

# @where: Hide style: pixelate
# @kind: option
# @max: 12
hide-pixels = Pixels

# @where: Hide style: solid plate
# @kind: option
# @max: 12
hide-plate = Solid

# @where: Slider: how strong the hiding is
# @kind: label
# @max: 16
hide-strength = Strength

# @where: Layer row of a hide mark with a detected kind; $what is e.g. "e-mail"
# @kind: label
# @max: 40
hide-hidden-name = Hidden: { $what }

# @where: Highlighter: band height
# @kind: label
# @max: 16
mark-band = Band height

# @where: Counter: badge shape
# @kind: label
# @max: 16
counter-shape = Shape

# @where: Counter shape
# @kind: option
# @max: 12
counter-circle = Circle

# @where: Counter shape
# @kind: option
# @max: 12
counter-square = Square

# @where: Counter shape: map-pin
# @kind: option
# @max: 12
counter-pin = Pin

# @where: Counter: colour of the number
# @kind: label
# @max: 16
counter-digit-colour = Number

# @where: Counter number colour option
# @kind: tooltip
# @max: 60
counter-digit-auto = Auto: black or white, whichever reads better

# @where: Inspector, Counter: caption of the counter's own colour row
# @kind: label
# @max: 20
counter-colour = Colour

# @where: Inspector, Counter: the button between the colour and the number rows
# @kind: tooltip
# @max: 48
counter-swap = Swap the colour and the number

# @where: Inspector, Counter: the row of fixed sizes S M L XL
# @kind: label
# @max: 12
counter-size = Size

# @where: Counter context menu
# @kind: menu
# @max: 32
counter-start-from = Start numbering from…

# @where: Counter context menu
# @kind: menu
# @max: 32
counter-new-group = New numbering group

# @where: Counter context menu
# @kind: menu
# @max: 48
counter-edit-group = Edit the whole group (colour, size, shape)

# @where: Counter context menu
# @kind: menu
# @max: 32
counter-delete-group = Delete group

# @where: Counter inspector: the number the next click will place
# @kind: hint
# @max: 16
counter-next = Next: { $n }

# @where: Layer row name of a counter
# @kind: label
# @max: 20
counter-name = Counter { $n }

# @where: Stamp and emoji picker (tool S)
# @kind: heading
# @max: 40
stamp-picker = Stamps and emoji

# @where: Search field of the picker
# @kind: placeholder
# @max: 24
stamp-search = Search emoji

# @where: Picker tab with Znimok stamps
# @kind: tab
# @max: 12
stamp-stamps = Stamps

# @where: Stamp picker: heading of the emoji grid
# @kind: label
# @max: 16
stamp-emoji = Emoji

# @where: Picker section
# @kind: tab
# @max: 24
stamp-emoji-recent = Emoji · recent

# @where: Stamp name (accessible name)
# @kind: a11y
# @max: 20
stamp-check = Check mark

# @where: Stamp name (accessible name)
# @kind: a11y
# @max: 20
stamp-cross = Cross

# @where: Stamp name (accessible name)
# @kind: a11y
# @max: 20
stamp-question = Question

# @where: Stamp name (accessible name)
# @kind: a11y
# @max: 20
stamp-exclamation = Exclamation

# @where: Stamp name (accessible name)
# @kind: a11y
# @max: 20
stamp-star = Star

# @where: Stamp name (accessible name)
# @kind: a11y
# @max: 20
stamp-warning = Warning

## Layers
## The Layers tab of the inspector.


# @where: Heading of the list; order is top to bottom
# @kind: label
# @max: 40
layers-count =
    { $count ->
        [one] { $count } annotation · top to bottom
       *[other] { $count } annotations · top to bottom
    }

# @where: Button: group the selected marks
# @kind: a11y
# @max: 20
layers-group = Group

# @where: Button: ungroup
# @kind: a11y
# @max: 20
layers-ungroup = Ungroup

# @where: Button: one step up in z-order
# @kind: a11y
# @max: 20
layers-up = Bring forward

# @where: Button: one step down in z-order
# @kind: a11y
# @max: 20
layers-down = Send backward

# @where: Eye button of a hidden row
# @kind: a11y
# @max: 20
layers-show = Show

# @where: Eye button of a visible row
# @kind: a11y
# @max: 20
layers-hide = Hide

# @where: Chevron of a group row
# @kind: a11y
# @max: 24
layers-collapse-group = Collapse group

# @where: Chevron of a group row
# @kind: a11y
# @max: 24
layers-expand-group = Expand group

# @where: Row of a named group
# @kind: label
# @max: 40
layers-group-name = Group “{ $name }”

# @where: Default name of a new group
# @kind: label
# @max: 24
layers-group-default = Group { $n }

# @where: Last row: the screenshot itself
# @kind: label
# @max: 48
layers-background = Screenshot (background) — always at the bottom

# @where: Grey note under the list
# @kind: hint
# @max: 160
layers-hint = Drag to reorder; click selects, F2 renames; a group is one row, its members stay together.

# @where: Layers tab with no marks
# @kind: hint
# @max: 60
layers-empty = No annotations yet — draw something on the canvas.

# @where: Layers panel, under the list: how to reorder and group by dragging
# @kind: hint
# @max: 90
layers-drag-hint = Drag a row to change the order; drop it onto another row to group them.

## Arrange
## Align, distribute and z-order actions (context bar with several marks, menus).


# @where: Several marks selected
# @kind: tooltip
# @max: 32
arrange-align-left = Align left edges

# @where: Several marks selected
# @kind: tooltip
# @max: 32
arrange-align-hcentre = Align vertical centres

# @where: Several marks selected
# @kind: tooltip
# @max: 32
arrange-align-right = Align right edges

# @where: Several marks selected
# @kind: tooltip
# @max: 32
arrange-align-top = Align top edges

# @where: Several marks selected
# @kind: tooltip
# @max: 32
arrange-align-vcentre = Align horizontal centres

# @where: Several marks selected
# @kind: tooltip
# @max: 32
arrange-align-bottom = Align bottom edges

# @where: Three or more selected
# @kind: tooltip
# @max: 32
arrange-distribute-x = Even gaps across

# @where: Three or more selected
# @kind: tooltip
# @max: 32
arrange-distribute-y = Even gaps down

# @where: Context menu; shortcut ]
# @kind: menu
# @max: 28
arrange-front = Bring to front

# @where: Context menu; shortcut [
# @kind: menu
# @max: 28
arrange-back = Send to back

# @where: Context menu; Ctrl+G
# @kind: menu
# @max: 20
arrange-group = Group

# @where: Context menu; Ctrl+Shift+G
# @kind: menu
# @max: 20
arrange-ungroup = Ungroup

# @where: Object panel: section with order, align, spread and group controls
# @kind: label
# @max: 16
arrange-title = Arrange

# @where: Object panel: row label of the z-order buttons
# @kind: label
# @max: 12
arrange-order = Order

# @where: Object panel: row label of the six align buttons
# @kind: label
# @max: 12
arrange-align = Align

# @where: Object panel: row label of the even-gaps buttons
# @kind: label
# @max: 12
arrange-distribute = Spread

# @where: Context menu: new marks of this kind use this style
# @kind: menu
# @max: 40
arrange-default-style = Make default style

# @where: Status bar with several marks selected
# @kind: status
# @max: 24
arrange-selected = Selected: { $count }

## Image tab
## The Image tab of the inspector: source, crop, rotation, tone, image and canvas size.


# @where: Source line: window capture; $title is the window title
# @kind: label
# @max: 48
shot-source-window = Window “{ $title }”

# @where: Source line: whole display; $n is its number
# @kind: label
# @max: 48
shot-source-display = Display { $n }

# @where: Source line: region capture
# @kind: label
# @max: 48
shot-source-region = Region

# @where: Source line: pasted image
# @kind: label
# @max: 48
shot-source-clipboard = From clipboard

# @where: Source line: opened file
# @kind: label
# @max: 48
shot-source-file = File “{ $name }”

# @where: Facts line: an HDR capture tone-mapped to SDR; $nits is a number
# @kind: label
# @max: 48
shot-hdr = HDR → SDR, white { $nits } nits

# @where: Facts line: marks left outside the crop (they are kept)
# @kind: label
# @max: 40
shot-outside =
    { $count ->
        [one] { $count } outside the crop
       *[other] { $count } outside the crop
    }

# @where: Section: crop
# @kind: label
# @max: 12
shot-crop = Crop

# @where: Crop aspect: free
# @kind: option
# @max: 12
shot-crop-free = Free

# @where: Starts crop mode; (C) is the shortcut
# @kind: button
# @max: 20
shot-crop-button = Crop (C)

# @where: Section: rotate and mirror
# @kind: label
# @max: 28
shot-rotate-mirror = Rotate and mirror

# @where: Button
# @kind: a11y
# @max: 24
shot-rotate-left = Rotate left

# @where: Button
# @kind: a11y
# @max: 24
shot-rotate-right = Rotate right

# @where: Button
# @kind: a11y
# @max: 32
shot-mirror-h = Mirror horizontally

# @where: Button
# @kind: a11y
# @max: 32
shot-mirror-v = Mirror vertically

# @where: Section: tone
# @kind: label
# @max: 12
shot-tone = Tone

# @where: Hold to see the original
# @kind: button
# @max: 16
shot-compare = Compare

# @where: Tooltip of Compare
# @kind: tooltip
# @max: 60
shot-compare-tip = Hold to see the original frame

# @where: Tone slider; value in EV
# @kind: label
# @max: 14
shot-exposure = Exposure

# @where: Tone slider: midtones
# @kind: label
# @max: 14
shot-gamma = Gamma

# @where: Tone slider
# @kind: label
# @max: 14
shot-contrast = Contrast

# @where: Reset button of the tone section
# @kind: tooltip
# @max: 60
shot-tone-reset-tip = Back to the tone as captured

# @where: Grey note under the tone sliders
# @kind: hint
# @max: 120
shot-tone-hint = A recipe over the original: the original stays untouched; Compare shows it while you hold.

# @where: Opens the image size dialog
# @kind: button
# @max: 20
shot-image-size = Image…

# @where: Opens the canvas size dialog
# @kind: button
# @max: 20
shot-canvas-size = Canvas…

# @where: Grey note under the two buttons
# @kind: hint
# @max: 120
shot-bake-hint = Both bake the recipe into a new original; the previous one stays for undo.

# @where: Dialog title
# @kind: title
# @max: 32
size-image-title = Image size

# @where: Dialog title
# @kind: title
# @max: 32
size-canvas-title = Canvas size

# @where: Dialog: current size
# @kind: label
# @max: 32
size-now = Now: { $width } × { $height }

# @where: Dialog field
# @kind: label
# @max: 12
size-percent = Percent

# @where: Dialog checkbox
# @kind: label
# @max: 32
size-keep-ratio = Keep proportions

# @where: Dialog checkbox (image size only)
# @kind: label
# @max: 48
size-scale-text = Scale text with the image

# @where: Dialog checkbox: nearest-neighbour resampling
# @kind: label
# @max: 40
size-sharp = Sharp (no smoothing)

# @where: Dialog main button
# @kind: button
# @max: 16
size-apply = Resize

# @where: Dialog note (image size)
# @kind: hint
# @max: 160
size-note-image = Line widths, counters and stamps keep their size; text scales only with the checkbox above.

# @where: Dialog note (canvas size)
# @kind: hint
# @max: 160
size-note-canvas = The screenshot becomes an object on a larger or smaller canvas; nothing is stretched.

## Meta tab
## Title, description, author and the rest of the document metadata.


# @where: Field
# @kind: label
# @max: 16
meta-title = Title

# @where: Field
# @kind: label
# @max: 16
meta-description = Description

# @where: Field
# @kind: label
# @max: 16
meta-author = Author

# @where: Field: copyright notice
# @kind: label
# @max: 16
meta-rights = Copyright

# @where: Field
# @kind: label
# @max: 16
meta-tags = Tags

# @where: Placeholder of the new-tag field
# @kind: placeholder
# @max: 16
meta-add-tag = add…

# @where: Accessible name of the new-tag field
# @kind: a11y
# @max: 16
meta-add-tag-a11y = Add tag

# @where: Field: capture date and time
# @kind: label
# @max: 20
meta-taken = Date taken

# @where: Field: where it came from
# @kind: label
# @max: 16
meta-source = Source

# @where: Section: what export does with metadata
# @kind: label
# @max: 20
meta-on-export = On export

# @where: Checkbox
# @kind: label
# @max: 32
meta-write = Write to PNG / JPEG

# @where: Checkbox: remove everything
# @kind: label
# @max: 32
meta-strip = Remove all metadata

# @where: Grey note
# @kind: hint
# @max: 120
meta-program-note = The Program field is set to Znimok and its version, except in the remove-all mode.

## Crop mode
## The editor while the crop frame is being edited.


# @where: Crop bar label
# @kind: label
# @max: 12
crop-title = Crop

# @where: Lock icon next to the proportions
# @kind: a11y
# @max: 32
crop-lock-ratio = Lock proportions

# @where: Status hint after "Enter"
# @kind: hint
# @max: 16
crop-hint-done = done

# @where: Status hint after "Esc"
# @kind: hint
# @max: 16
crop-hint-cancel = cancel

# @where: Status hint
# @kind: hint
# @max: 40
crop-hint-move = drag inside — move the frame

# @where: Frame width field
# @kind: a11y
# @max: 24
crop-width = Crop width

# @where: Frame height field
# @kind: a11y
# @max: 24
crop-height = Crop height

# @where: Inspector line: original size → frame size
# @kind: label
# @max: 48
crop-sizes = { $width } × { $height } → crop { $cw } × { $ch }

# @where: Inspector line: marks outside the new frame are kept
# @kind: hint
# @max: 60
crop-outside-kept =
    { $count ->
        [one] { $count } annotation outside the crop — it will be kept
       *[other] { $count } annotations outside the crop — they will be kept
    }

# @where: Grey note while cropping
# @kind: hint
# @max: 160
crop-dimmed-note = The rest of the inspector is dimmed while cropping. The crop is a document property: annotations outside it are not deleted.

# @where: Status bar while cropping
# @kind: status
# @max: 48
crop-status =
    { $count ->
        [one] cropping · { $count } annotation outside
       *[other] cropping · { $count } annotations outside
    }

## Canvas and status bar
## Context menu of the canvas, drag-and-drop, status bar.


# @where: Context menu; Ctrl+V
# @kind: menu
# @max: 24
canvas-paste = Paste

# @where: Context menu; Ctrl+A
# @kind: menu
# @max: 24
canvas-select-all = Select all

# @where: Context menu; Ctrl+0
# @kind: menu
# @max: 24
canvas-fit = Fit

# @where: Context menu; Ctrl+1
# @kind: menu
# @max: 24
canvas-zoom-100 = Zoom 100 %

# @where: Drop overlay while a file is dragged over the canvas
# @kind: title
# @max: 48
canvas-drop-add = Release to add as an annotation

# @where: Drop overlay second line; { $key } is "Shift"
# @kind: hint
# @max: 60
canvas-drop-shift = Hold { $key } to open as a new screenshot

# @where: Empty canvas
# @kind: hint
# @max: 100
canvas-blank-hint = Blank canvas. Ctrl+V pastes an image; drop a file to open it.

# @where: Status bar button
# @kind: a11y
# @max: 24
status-fit = Fit to window

# @where: Status bar zoom value field
# @kind: a11y
# @max: 16
status-zoom = Zoom

# @where: Status bar button
# @kind: a11y
# @max: 24
status-zoom-100 = Zoom 100 %

# @where: Status bar: no crop
# @kind: status
# @max: 32
status-crop-whole = Crop: whole screenshot

# @where: Status bar: current crop
# @kind: status
# @max: 32
status-crop-size = Crop: { $width } × { $height }

# @where: Status bar after a save
# @kind: status
# @max: 32
status-saved-library = Saved to library

## Capture overlay
## The frozen-screen overlay for choosing what to capture. Key names (Shift, Alt, Space, Esc) are keys: keep them.


# @where: Hint chip: releasing the mouse without a key
# @kind: badge
# @max: 12
capture-release = release

# @where: Hint: where the capture goes on release
# @kind: hint
# @max: 20
capture-to-editor = to editor

# @where: Hint after "Shift"
# @kind: hint
# @max: 20
capture-to-clipboard = to clipboard

# @where: Hint after "Alt"
# @kind: hint
# @max: 20
capture-over-screen = over the screen

# @where: Hint after "Space"
# @kind: hint
# @max: 20
capture-whole-screen = whole screen

# @where: Hint chip: a click (not a drag)
# @kind: badge
# @max: 12
capture-click = click

# @where: Hint after "click"
# @kind: hint
# @max: 20
capture-window = window

# @where: Hint chip: the mouse wheel / two-finger scroll
# @kind: badge
# @max: 12
capture-wheel = wheel

# @where: Hint after the wheel chip: it turns the magnifier on and zooms it
# @kind: hint
# @max: 20
capture-magnifier = magnifier

# @where: Hint chip: a second click (a double click, or a click and then a drag)
# @kind: badge
# @max: 16
capture-double = double click

# @where: Hint after that chip: the shot is taken after a 3-2-1 countdown
# @kind: hint
# @max: 20
capture-delayed = after 3 s

# @where: Hint after the "Q" chip: read QR codes and barcodes in the selection or the screen
# @kind: hint
# @max: 20
capture-qr = QR code

# @where: Hint after the "S" chip: a scrolling capture of the highlighted window or region
# @kind: hint
# @max: 20
capture-scroll = with scrolling

# @where: Hint after "Esc"
# @kind: hint
# @max: 20
capture-cancel = cancel

# @where: Capture overlay of a recording, hint strip: what releasing does
# @kind: hint
# @max: 20
capture-record = record

# @where: Capture overlay of a recording, hint strip beside the A key: $mode is the sound choice in lower case («system sound»)
# @kind: hint
# @max: 40
capture-sound = sound: { $mode }

# @where: Mode switch, top right
# @kind: option
# @max: 12
capture-mode-shot = Screenshot

# @where: Mode switch, top right
# @kind: option
# @max: 12
capture-mode-record = Record

# @where: Countdown before a delayed capture: Esc cancels
# @kind: hint
# @max: 24
capture-countdown-cancel = Esc — cancel

# @where: Scrolling capture panel: the height stitched so far; $height is in pixels
# @kind: status
# @max: 40
scroll-progress = Scrolling… { $height } px

# @where: Scrolling capture panel, when automatic scrolling does not move the content
# @kind: hint
# @max: 48
scroll-manual = Scroll it yourself — Znimok keeps up

# @where: Scrolling capture panel, under the height while Znimok scrolls
# @kind: hint
# @max: 48
scroll-auto = Esc — cancel

## Overlay editor
## Editing right over the frozen screen (Alt on release).


# @where: Chip at the left of the overlay bar
# @kind: badge
# @max: 20
overlay-chip = Over the screen

# @where: Button: move to the editor window
# @kind: a11y
# @max: 32
overlay-open-window = Open in the editor window

# @where: Tooltip of the same button
# @kind: tooltip
# @max: 80
overlay-open-window-tip = To the editor window — tone, size, library

# @where: Hint in the overlay status
# @kind: hint
# @max: 48
overlay-hint-frame = drag the frame corners — that is the crop

# @where: Hint after "Ctrl+S"
# @kind: hint
# @max: 20
overlay-hint-save = to library

# @where: Hint after "Esc"
# @kind: hint
# @max: 16
overlay-hint-close = close

## Pill
## The small card in the corner of the display after a capture (6 s).


# @where: Pill title after a region capture
# @kind: title
# @max: 40
pill-region-copied = Region screenshot copied

# @where: Pill title after a whole-screen capture
# @kind: title
# @max: 40
pill-screen-copied = Screenshot copied

# @where: Pill title after a window capture
# @kind: title
# @max: 40
pill-window-copied = Window screenshot copied

# @where: Pill title when the capture only went to the library
# @kind: title
# @max: 40
pill-saved = Saved to library

# @where: Pill second line
# @kind: hint
# @max: 48
pill-where = { $width } × { $height } · in the clipboard and the library

# @where: Pill button
# @kind: button
# @max: 14
pill-edit = Edit

# @where: Pill "…" button
# @kind: a11y
# @max: 80
pill-more = More: save as file, hand to agent, show in library

# @where: Pill menu
# @kind: menu
# @max: 28
pill-save-file = Save as file…

# @where: Pill menu
# @kind: menu
# @max: 28
pill-show-library = Show in library

# @where: Yellow pill while an agent captures; $client is its name
# @kind: title
# @max: 48
pill-agent-capturing = { $client } is capturing the screen

## Tray
## The tray menu on Windows and the menu bar menu on macOS (same items, native menu).


# @where: Tray menu
# @kind: menu
# @max: 28
tray-region = Region screenshot

# @where: Tray menu
# @kind: menu
# @max: 28
tray-screen = Screenshot

# @where: Tray menu
# @kind: menu
# @max: 28
tray-record = Record video

# @where: Tray menu
# @kind: menu
# @max: 28
tray-open = Open Znimok

# @where: Tray / menu bar item: read QR codes and barcodes on the screen under the pointer
# @kind: menu
# @max: 40
tray-read-codes = Read a QR code from the screen

# @where: Tray menu: release all hotkeys for a while
# @kind: menu
# @max: 32
tray-pause-keys = Pause hotkeys

# @where: Tray menu while paused
# @kind: menu
# @max: 32
tray-resume-keys = Resume hotkeys

# @where: Tray menu (Windows)
# @kind: menu
# @max: 20
tray-quit = Quit

# @where: Menu bar menu (macOS)
# @kind: menu
# @max: 24
tray-quit-mac = Quit Znimok

# @where: Tray icon tooltip when idle
# @kind: tooltip
# @max: 40
tray-tooltip = Znimok

# @where: Tray icon tooltip while recording; $time like 00:12
# @kind: tooltip
# @max: 40
tray-tooltip-recording = Znimok — recording { $time }

# @where: Tray icon tooltip while hotkeys are paused
# @kind: tooltip
# @max: 40
tray-tooltip-paused-keys = Znimok — hotkeys paused

## Export dialog
## Export a file (Ctrl+Shift+S).


# @where: Dialog title and accessible name
# @kind: title
# @max: 24
export-title = Export file

# @where: Left column heading
# @kind: label
# @max: 20
export-preview = Preview

# @where: Fact label
# @kind: label
# @max: 12
export-size = Size

# @where: Fact label: estimated file size
# @kind: label
# @max: 12
export-file = File

# @where: Fact label: number of annotations
# @kind: label
# @max: 12
export-marks = Annotations

# @where: Note under the preview
# @kind: hint
# @max: 140
export-flat-note = A file with annotations is a flat image. The editable project stays in the library.

# @where: Field
# @kind: label
# @max: 12
export-format = Format

# @where: Under the format choice
# @kind: hint
# @max: 120
export-format-hint = PNG — lossless, with transparency. JPEG and WebP — smaller files for photos and chats.

# @where: Field
# @kind: label
# @max: 12
export-scale = Scale

# @where: Field (JPEG/WebP)
# @kind: label
# @max: 12
export-quality = Quality

# @where: Checkbox
# @kind: label
# @max: 56
export-metadata = Write metadata (title, description, author, date)

# @where: Item of the Copy button's menu; toggles writing title, description, author and date into files
# @kind: menu
# @max: 28
export-metadata-menu = Write metadata

# @where: Checkbox
# @kind: label
# @max: 32
export-white-bg = Transparent background → white

# @where: Checkbox
# @kind: label
# @max: 32
export-remember = Remember these settings

# @where: File name field
# @kind: label
# @max: 12
export-name = Name

# @where: Accessible name of the file name field
# @kind: a11y
# @max: 16
export-name-a11y = File name

# @where: Dialog secondary button
# @kind: button
# @max: 16
export-to-clipboard = To clipboard

# @where: Toast after export; $name is the file name
# @kind: toast
# @max: 60
export-done-toast = Exported “{ $name }”

# @where: Toast when the file cannot be written
# @kind: error
# @max: 80
export-error = Could not save the file.

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
export-png-sub = lossless

# @where: Export sheet: JPEG card, beside the title
# @kind: label
# @max: 20
export-jpeg-sub = with quality

# @where: Export sheet: WebP card, beside the title
# @kind: label
# @max: 20
export-webp-sub = smallest files

# @where: Export sheet: PNG card, what it is for
# @kind: hint
# @max: 90
export-png-desc = Every pixel as it is, with transparency — for interfaces and text.

# @where: Export sheet: JPEG card, what it is for
# @kind: hint
# @max: 90
export-jpeg-desc = The smallest for photos and gradients; no transparency.

# @where: Export sheet: WebP card, what it is for
# @kind: hint
# @max: 90
export-webp-desc = The smallest files, with a quality or lossless; opens in browsers and chats.

# @where: Export sheet, WebP chosen: a switch; off = the quality slider (libwebp)
# @kind: label
# @max: 30
export-lossless = Lossless

# @where: Export sheet: under the title — the picture's size and the size it will have
# @kind: status
# @max: 80
export-subtitle = { $w } × { $h } → { $tw } × { $th } px · marks drawn in

# @where: Export sheet: caption of the row «To clipboard / File… / To the library»
# @kind: label
# @max: 16
export-where = Where

# @where: Export sheet: where to — a file (the save dialog comes next)
# @kind: button
# @max: 16
export-to-file = File…

# @where: Export sheet: where to — a flat copy in the library
# @kind: button
# @max: 20
export-to-library = To the library

# @where: Export sheet: note when the clipboard is chosen
# @kind: hint
# @max: 100
export-clipboard-note = The clipboard takes the picture itself; the format and quality are for files.

# @where: Export sheet: the main button for a file
# @kind: button
# @max: 24
export-go = Export { $format }

# @where: Export sheet: the main button for the clipboard
# @kind: button
# @max: 24
export-go-copy = Copy

# @where: Export sheet: the main button for the library
# @kind: button
# @max: 24
export-go-library = Save to the library

# @where: Toast: the flat copy went to the library
# @kind: toast
# @max: 48
export-library-done = A flat copy is in the library

# @where: A file size in kilobytes
# @kind: label
# @max: 12
size-kb = { $n } KB

# @where: A file size in megabytes
# @kind: label
# @max: 12
size-mb = { $n } MB

# @where: Toast after copying
# @kind: toast
# @max: 24
clipboard-copied = Copied

# @where: Title of the list of codes found; $count is how many
# @kind: title
# @max: 40
codes-title = Codes found: { $count }

# @where: Title when no code was found
# @kind: title
# @max: 40
codes-none-title = No codes

# @where: Body when no code was found
# @kind: body
# @max: 120
codes-none-body = There are no QR codes or barcodes on this picture.

# @where: A found code: a web link
# @kind: body
# @max: 200
codes-link = Link: { $url }

# @where: A found code: a Wi-Fi network with a password
# @kind: body
# @max: 200
codes-wifi = Wi-Fi “{ $ssid }” · password: { $password } · { $security }

# @where: A found code: a Wi-Fi network without a password
# @kind: body
# @max: 120
codes-wifi-open = Wi-Fi “{ $ssid }” without a password

# @where: A found code: a contact card
# @kind: label
# @max: 32
codes-contact = Contact

# @where: A found code: a calendar event
# @kind: label
# @max: 32
codes-event = Event

# @where: A found code: an e-mail address
# @kind: body
# @max: 120
codes-email = E-mail: { $address }

# @where: A found code: a phone number
# @kind: body
# @max: 80
codes-phone = Phone: { $number }

# @where: A found code: plain text
# @kind: body
# @max: 200
codes-text = Text: { $text }

# @where: Button: copy the text of the codes
# @kind: button
# @max: 24
codes-copy = Copy text

# @where: Button: open the link of a code (a confirmation follows)
# @kind: button
# @max: 24
codes-open-link = Open link…

# @where: Question before a link from a code opens in the browser
# @kind: title
# @max: 40
codes-open-title = Open this link?

# @where: Body of that question; $url is the whole address
# @kind: body
# @max: 400
codes-open-body = { $url } — check the whole address: QR codes are often used for phishing.

# @where: Button: open the link
# @kind: button
# @max: 16
codes-open = Open

# @where: Editor, Image tab: button to read QR codes and barcodes on the picture
# @kind: button
# @max: 28
img-read-codes = Read QR codes and barcodes

# @where: Image tab: button that reads the text on the screenshot (on the device, no AI)
# @kind: button
# @max: 32
img-read-text = Text from the screenshot

# @where: Title of the panel with the text found on the screenshot
# @kind: heading
# @max: 32
text-title = Text on the screenshot

# @where: Text panel status while the text is being read
# @kind: status
# @max: 40
text-busy = Reading the text…

# @where: Text panel status: how many lines were found (read on this device)
# @kind: status
# @max: 48
text-count = { $n } lines · read on this device

# @where: Text panel status: nothing was found
# @kind: status
# @max: 40
text-none = No text found

# @where: Text panel status: languages the system cannot read yet
# @kind: status
# @max: 80
text-missing = not installed for reading: { $langs }

# @where: Text panel status (Windows): no reader for Ukrainian — neither Znimok's helper nor the Windows language pack
# @kind: status
# @max: 200
text-missing-uk = Ukrainian cannot be read here, Cyrillic may come out wrong: reinstall Znimok (its text reader comes with it) or add the Ukrainian language pack to Windows

# @where: Text panel status: the reading failed
# @kind: error
# @max: 96
text-error = Could not read the text: { $error }

# @where: Text panel: copies the whole text as shown (after any corrections)
# @kind: button
# @max: 20
text-copy-all = Copy all

# @where: Text panel: hint under the text
# @kind: hint
# @max: 120
text-hint = Drag a frame over the picture to read only that part; click a line to copy it.

# @where: Tray menu item: choose a part of the screen, its text goes to the clipboard
# @kind: menu
# @max: 40
tray-read-text = Copy text from the screen

# @where: Hotkeys settings row: choose a part of the screen, its text goes to the clipboard
# @kind: label
# @max: 40
keys-read-text = Copy text from the screen

# @where: Question after a quick text reading: the text is on the clipboard; the body shows it
# @kind: heading
# @max: 48
text-copied-title = Text copied — { $n } lines

# @where: Toast when the clipboard is locked or fails
# @kind: error
# @max: 80
clipboard-error = Could not copy to the clipboard.

# @where: Footer of the self-contained HTML page of a screenshot; $version is the Znimok version
# @kind: hint
# @max: 48
html-made-with = Made with Znimok { $version }

## Library
## The home screen: grid of screenshots and videos, filters, details panel.


# @where: Page title
# @kind: title
# @max: 24
lib-title = Library

# @where: Search field
# @kind: placeholder
# @max: 60
lib-search = Search by name, tag or text in the screenshot

# @where: Main button
# @kind: button
# @max: 20
lib-new-shot = New screenshot

# @where: Arrow next to New screenshot
# @kind: a11y
# @max: 32
lib-capture-more = Other capture options

# @where: Button (v2)
# @kind: button
# @max: 16
lib-record = Record

# @where: Filter
# @kind: option
# @max: 12
lib-filter-all = All

# @where: Filter
# @kind: option
# @max: 14
lib-filter-shots = Screenshots

# @where: Filter
# @kind: option
# @max: 12
lib-filter-videos = Videos

# @where: Filter: videos with a DevTools report
# @kind: option
# @max: 16
lib-filter-report = With report

# @where: Sort menu
# @kind: option
# @max: 20
lib-sort-newest = Newest first

# @where: Sort menu
# @kind: option
# @max: 20
lib-sort-oldest = Oldest first

# @where: Sort menu
# @kind: option
# @max: 20
lib-sort-name = By name

# @where: Sort menu
# @kind: option
# @max: 20
lib-sort-size = By size

# @where: Card badge of a recording with a browser log
# @kind: badge
# @max: 16
lib-badge-devtools = DevTools log

# @where: Details panel chip
# @kind: button
# @max: 12
lib-add-tag = + tag

# @where: Details panel main button; also card menu (Enter)
# @kind: button
# @max: 24
lib-open-editor = Open in editor

# @where: Details panel short button
# @kind: button
# @max: 12
lib-to-agent = To agent

# @where: Details panel: show the file in Explorer / Finder
# @kind: button
# @max: 16
lib-in-folder = In folder

# @where: Card context menu
# @kind: menu
# @max: 28
lib-show-in-folder = Show in folder

# @where: Card context menu
# @kind: menu
# @max: 24
lib-trash = Move to trash

# @where: Details panel button
# @kind: button
# @max: 24
lib-trash-long = Move to trash

# @where: Tooltip of the card's trash button, after "Shift —": Shift+click skips the trash
# @kind: tooltip
# @max: 24
lib-delete-forever = delete for good

# @where: Question before Shift+click deletes a document without the trash
# @kind: title
# @max: 40
lib-delete-forever-title = Delete for good?

# @where: Body of that question; $name is the document's name
# @kind: body
# @max: 160
lib-delete-forever-body = “{ $name }” will be deleted without the trash. This cannot be undone.

# @where: Status line after a document was deleted for good
# @kind: toast
# @max: 40
lib-deleted-forever-toast = Deleted for good

# @where: Details fact
# @kind: label
# @max: 16
lib-source = Source

# @where: Source value
# @kind: value
# @max: 16
lib-src-window = Window

# @where: Source value
# @kind: value
# @max: 16
lib-src-display = Display

# @where: Details fact
# @kind: label
# @max: 12
lib-file = File

# @where: Toast after deleting, with an Undo action
# @kind: toast
# @max: 48
lib-trashed-toast = Screenshot moved to trash

# @where: Toast action
# @kind: button
# @max: 16
lib-undo = Undo

# @where: Toast
# @kind: error
# @max: 60
lib-error-delete = Could not delete the file.

# @where: Toast
# @kind: error
# @max: 60
lib-error-rename = Could not rename the screenshot.

# @where: Card badge: this item is open in the editor
# @kind: badge
# @max: 16
lib-in-editor = in the editor

# @where: Library status bar
# @kind: status
# @max: 60
lib-status =
    { $count ->
        [one] { $count } item · { $size }
       *[other] { $count } items · { $size }
    }

# @where: Empty library, first run
# @kind: title
# @max: 48
lib-empty-title = Your screenshots will appear here

# @where: Empty library; $key is the region hotkey, e.g. Alt+Shift+4
# @kind: body
# @max: 120
lib-empty-body = Press { $key } to capture a region, or use the button below.

# @where: Empty library, under the button
# @kind: hint
# @max: 60
lib-empty-drop = Drop an image here to open it

# @where: Search without results; $query is what the user typed
# @kind: title
# @max: 60
lib-search-empty-title = Nothing found for “{ $query }”

# @where: Search without results
# @kind: body
# @max: 160
lib-search-empty-body =
    { $count ->
        [one] We search names, tags and text in screenshots. Text recognition is off for { $count } older screenshot.
       *[other] We search names, tags and text in screenshots. Text recognition is off for { $count } older screenshots.
    }

# @where: Search without results
# @kind: button
# @max: 28
lib-ocr-all = Recognise text in all

## Settings
## The Settings page inside the main window: navigation and shared rows.


# @where: Page title
# @kind: title
# @max: 24
set-title = Settings

# @where: Search field
# @kind: placeholder
# @max: 28
set-search = Find a setting

# @where: Note next to the title
# @kind: hint
# @max: 32
set-applied-now = Changes apply immediately

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-shots = Screenshots

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-keys = Hotkeys

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-recording = Recording

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-library = Library

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-sharing = Sharing

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-agents = Agents and models

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-look = Appearance and language

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-privacy = Privacy

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-updates = Updates

# @where: Settings navigation
# @kind: tab
# @max: 24
set-page-about = About Znimok

## Settings · Hotkeys
## Global hotkeys page.


# @where: Page intro
# @kind: body
# @max: 200
keys-intro = They work in any program. Click a field and press a combination to change it. On a Mac the standard ⌘⇧3/4/5 belong to the system, so the defaults are different.

# @where: Master switch
# @kind: label
# @max: 32
keys-enable = Capture with hotkeys

# @where: Under the master switch
# @kind: hint
# @max: 80
keys-enable-hint = Off — the combinations are free for other programs

# @where: Row
# @kind: label
# @max: 32
keys-region = Region screenshot

# @where: Row
# @kind: label
# @max: 32
keys-screen = Whole-screen screenshot

# @where: Row
# @kind: label
# @max: 32
keys-window = Window screenshot

# @where: Row
# @kind: label
# @max: 32
keys-record = Video recording: start / stop

# @where: Row
# @kind: label
# @max: 32
keys-clipboard = Clipboard image → editor

# @where: Row
# @kind: label
# @max: 32
keys-blank = Blank editor

# @where: Row on the Hotkeys settings page (ZK-146)
# @kind: label
# @max: 32
keys-read-codes = Read QR codes on the screen

# @where: Field while waiting for a combination
# @kind: placeholder
# @max: 28
keys-press = press a combination…

# @where: Field without a combination
# @kind: value
# @max: 16
keys-not-set = not set

# @where: Under a field when the new combination is taken
# @kind: error
# @max: 60
keys-taken = Held by another program — kept the previous one

# @where: Page footer
# @kind: button
# @max: 24
keys-defaults = Restore defaults

# @where: Page footer: canvas navigation
# @kind: hint
# @max: 60
keys-canvas-hint = Zoom: Alt + wheel · Pan: Space + drag

## Settings · Screenshots
## What happens when a capture is released, the pill, editor behaviour.


# @where: Page intro
# @kind: body
# @max: 120
shots-intro = What happens when you release the mouse in the overlay. Each action sits on exactly one gesture.

# @where: Section title
# @kind: label
# @max: 48
shots-gestures = What a capture does on release

# @where: Under the section title
# @kind: hint
# @max: 100
shots-gestures-hint = Each action is bound to exactly one gesture. Drag to swap.

# @where: Gesture chip: release without a key
# @kind: badge
# @max: 16
shots-no-key = no key

# @where: Action
# @kind: option
# @max: 28
shots-act-editor = Open in editor

# @where: Action
# @kind: option
# @max: 28
shots-act-clipboard = To clipboard

# @where: Action
# @kind: option
# @max: 28
shots-act-overlay = Edit over the screen

# @where: Row: the pill after a capture
# @kind: label
# @max: 32
shots-pill = Card after a capture

# @where: Duration option; $n is a number
# @kind: option
# @max: 8
shots-seconds = { $n } s

# @where: Switch
# @kind: label
# @max: 48
shots-quick-library = Quick captures go to the library too

# @where: Settings, Screenshots: the gesture of a row (releasing the mouse without a modifier)
# @kind: label
# @max: 18
shots-gesture-plain = Release

# @where: Settings, Screenshots: the gesture of a row (Shift held on release)
# @kind: label
# @max: 18
shots-gesture-shift = Shift + release

# @where: Settings, Screenshots: the gesture of a row (Alt held on release; Windows)
# @kind: label
# @max: 18
shots-gesture-alt = Alt + release

# @where: Settings, Screenshots: the gesture of a row (Option held on release; macOS)
# @kind: label
# @max: 18
shots-gesture-option = ⌥ + release

# @where: Switch
# @kind: label
# @max: 48
shots-show-hints = Show the hint strip while capturing

# @where: Settings, Screenshots: segment — the capture opens in the editor window
# @kind: label
# @max: 16
shots-seg-editor = Editor

# @where: Settings, Screenshots: segment — the capture opens in the editor right over the screen
# @kind: label
# @max: 16
shots-seg-over = Over the screen

# @where: Settings, Screenshots: segment — the capture goes straight to the clipboard
# @kind: label
# @max: 16
shots-seg-clipboard = Clipboard

# @where: Switch
# @kind: label
# @max: 60
shots-esc-saves = Esc over the screen saves if there are annotations

# @where: Switch
# @kind: label
# @max: 60
shots-tool-stays = The tool stays active after drawing

# @where: Switch (same as the title bar toggle)
# @kind: label
# @max: 60
shots-autosave = Save automatically (also in the editor title bar)

# @where: Switch
# @kind: label
# @max: 60
shots-guides = Smart guides: centres, edges, equal gaps

# @where: Row: what a left click on the tray icon does
# @kind: label
# @max: 48
shots-tray-click = Left click on the tray icon

# @where: Option
# @kind: option
# @max: 16
shots-tray-menu = Menu

# @where: Option
# @kind: option
# @max: 20
shots-tray-new = New screenshot

# @where: Switch
# @kind: label
# @max: 32
shots-cursor = Cursor in screenshots

## Settings · Recording
## Video recording settings (v2).


# @where: Page intro
# @kind: body
# @max: 120
rec-intro = Quality and sources. Sound is off by default: you turn it on deliberately.

# @where: Row
# @kind: label
# @max: 32
rec-fps = Frames per second

# @where: Row
# @kind: label
# @max: 16
rec-quality = Quality

# @where: Option
# @kind: option
# @max: 12
rec-quality-low = Lower

# @where: Option
# @kind: option
# @max: 12
rec-quality-normal = Normal

# @where: Option
# @kind: option
# @max: 12
rec-quality-high = High

# @where: Row
# @kind: label
# @max: 12
rec-codec = Codec

# @where: Row: what clicking a window does in record mode
# @kind: label
# @max: 32
rec-window-click = Click on a window

# @where: Option: the frame follows the window
# @kind: option
# @max: 16
rec-follow = follow it

# @where: Option: record its area only
# @kind: option
# @max: 16
rec-as-region = as a region

# @where: Row
# @kind: label
# @max: 24
rec-system-audio = System sound

# @where: Device picker default
# @kind: option
# @max: 24
rec-default-device = Default device

# @where: Row
# @kind: label
# @max: 24
rec-microphone = Microphone

# @where: Row
# @kind: label
# @max: 40
rec-cursor-clicks = Cursor and click highlight

# @where: Settings → Recording: the switch — a finished recording opens in the editor (off: only the card after it)
# @kind: label
# @max: 44
rec-open-editor = Open the editor when a recording ends

# @where: Row
# @kind: label
# @max: 40
rec-devtools = DevTools log from Chrome/Edge

# @where: Row status
# @kind: status
# @max: 32
rec-extension-connected = extension connected

## Settings · Library
## Library folder, retention, text recognition.


# @where: Page intro
# @kind: body
# @max: 120
libset-intro = The folder is the source of truth. You can keep it on a cloud drive and Znimok will pick up changes.

# @where: Row
# @kind: label
# @max: 24
libset-folder = Library folder

# @where: Settings, Library: the row with the two fields for the words before the date in new names (ZK-221)
# @kind: label
# @max: 32
libset-names = Names of new documents

# @where: Settings, Library: the label of the field with the word for new screenshots
# @kind: label
# @max: 16
libset-shot-prefix = Screenshots

# @where: Settings, Library: the label of the field with the word for new recordings
# @kind: label
# @max: 16
libset-video-prefix = Recordings

# @where: Settings, Library: the placeholder of the recordings' field — the usual word, as in rec-doc-name
# @kind: label
# @max: 16
libset-video-word = Recording

# @where: Settings, Library: the hint under the two fields
# @kind: body
# @max: 120
libset-names-hint = The word before the date and the time, as in «Screenshot 2026-10-02 12.00.00». Empty: the usual word.

# @where: Under the folder; $size is formatted
# @kind: hint
# @max: 60
libset-stats =
    { $count ->
        [one] { $count } screenshot · { $size }
       *[other] { $count } screenshots · { $size }
    }

# @where: Row
# @kind: label
# @max: 40
libset-keep-shots = Keep screenshots no more than

# @where: Option
# @kind: option
# @max: 12
libset-by-count = count

# @where: Option
# @kind: option
# @max: 12
libset-by-size = size

# @where: Row
# @kind: label
# @max: 32
libset-keep-videos = Videos — no more than

# @where: Switch
# @kind: label
# @max: 48
libset-oldest-trash = The oldest go to the trash, not away for good

# @where: Library header: a segment — show everything
# @kind: button
# @max: 10
lib-kind-all = All

# @where: Library header: a segment — screenshots only
# @kind: button
# @max: 12
lib-kind-shots = Shots

# @where: Library header: a segment — videos only
# @kind: button
# @max: 12
lib-kind-videos = Videos

# @where: Switch
# @kind: label
# @max: 60
libset-ocr = Recognise text in screenshots for search (on device)

# @where: Button
# @kind: button
# @max: 20
libset-show-folder = Show folder

# @where: Button
# @kind: button
# @max: 24
libset-clear = Clear library…

# @where: Confirmation dialog
# @kind: body
# @max: 120
libset-clear-confirm =
    { $count ->
        [one] Move { $count } item of the library to the trash?
       *[other] Move all { $count } items of the library to the trash?
    }

## Settings · Sharing
## What the Copy button does and the formats.


# @where: Page intro
# @kind: body
# @max: 120
sharing-intro = What the main Copy button does and which format comes out.

# @where: Row
# @kind: label
# @max: 32
sharing-main = Main button of the action row

# @where: Option
# @kind: option
# @max: 28
sharing-main-copy = Copy to clipboard

# @where: Row
# @kind: label
# @max: 24
sharing-format = Screenshot format

# @where: Switch
# @kind: label
# @max: 40
sharing-metadata = Write metadata into files

# @where: Switch
# @kind: label
# @max: 40
sharing-mask = Mask secrets in reports

# @where: Row
# @kind: label
# @max: 16
sharing-file-name = File name

# @where: Under the pattern field; the {date}/{time} tokens in braces are literal and must stay in English
# @kind: hint
# @max: 80
# @note: Shown as "{date}" and "{time}": literal braces (Fluent escapes). Keep the words date and time in English — they are tokens.
sharing-file-pattern-hint = Use { "{" }date{ "}" } and { "{" }time{ "}" } in the name

# @where: Share targets row
# @kind: hint
# @max: 60
sharing-targets-later = Slack, Jira, Telegram, Redmine — later

## Settings · Privacy
## Everything that touches the network and data.


# @where: Page intro
# @kind: body
# @max: 120
priv-intro = Znimok sends nothing anywhere without your action. Everything that touches the network and your data is here.

# @where: Switch
# @kind: label
# @max: 40
priv-updates = Check for updates daily

# @where: Under the switch
# @kind: hint
# @max: 48
priv-updates-hint = GitHub Releases, version number only

# @where: Row
# @kind: label
# @max: 32
priv-telemetry = Telemetry and statistics

# @where: Row value
# @kind: value
# @max: 40
priv-telemetry-none = none, and no switch either

# @where: Row
# @kind: label
# @max: 24
priv-crash = Crash reports

# @where: Row value
# @kind: value
# @max: 40
priv-crash-local = a local file; you send it yourself

# @where: Row
# @kind: label
# @max: 40
priv-cloud = Cloud models (your own key)

# @where: Row value
# @kind: value
# @max: 32
priv-cloud-off = off, no key

# @where: Row action
# @kind: button
# @max: 20
priv-add-key = Add key…

# @where: Row
# @kind: label
# @max: 40
priv-agents = Agent access (MCP)

# @where: Row value
# @kind: value
# @max: 40
priv-agents-count =
    { $count ->
        [one] { $count } client has permission
       *[other] { $count } clients have permission
    }

# @where: Row (macOS)
# @kind: label
# @max: 32
priv-mac-perms = macOS permissions

# @where: Row action
# @kind: button
# @max: 16
priv-check = Check

# @where: Link
# @kind: button
# @max: 28
priv-policy = Privacy policy

# @where: Link
# @kind: button
# @max: 24
priv-source = Source code

## Settings · Appearance and About
## Theme, language, launch at login, About.


# @where: Row
# @kind: label
# @max: 12
look-theme = Theme

# @where: Option
# @kind: option
# @max: 20
look-theme-system = As in the system

# @where: Option
# @kind: option
# @max: 12
look-theme-light = Light

# @where: Option
# @kind: option
# @max: 12
look-theme-dark = Dark

# @where: Row
# @kind: label
# @max: 12
look-language = Language

# @where: Option: follow the system
# @kind: option
# @max: 16
look-language-system = System

# @where: Name of THIS language in itself, for the language menu
# @kind: option
# @max: 20
# @note: Write the language's own name in the language itself (endonym).
look-language-name = English

# @where: Row
# @kind: label
# @max: 32
look-tool-labels = Labels under tools

# @where: Option
# @kind: option
# @max: 12
look-labels-always = always

# @where: Option
# @kind: option
# @max: 16
look-labels-first = first runs

# @where: Option
# @kind: option
# @max: 12
look-labels-never = never

# @where: Switch
# @kind: label
# @max: 40
look-autostart = Start at sign-in

# @where: Row
# @kind: label
# @max: 24
look-reduce-motion = Reduce motion

# @where: Row value
# @kind: value
# @max: 20
look-as-system = as in the system

# @where: About: version line
# @kind: label
# @max: 40
about-version = Znimok { $version }

# @where: About page, under the Znimok wordmark next to the app icon
# @kind: value
# @max: 32
about-version-line = Version { $version }

# @where: About: after the version
# @kind: badge
# @max: 40
about-signed = release signature verified

# @where: About: credits; Slint and font names stay as they are
# @kind: body
# @max: 200
about-made-with = Made with Slint · fonts Onest, JetBrains Mono, Unbounded (OFL) · library licences

# @where: About page: the bundled fonts and their licence (keep the font names)
# @kind: body
# @max: 90
about-fonts = Fonts: Onest, JetBrains Mono, Unbounded — SIL Open Font License 1.1

# @where: About page, under the Znimok wordmark: what the program is
# @kind: body
# @max: 70
about-tagline = Screenshots and screen video with annotations

# @where: About page, next to the version: copies the version and build for a bug report
# @kind: button
# @max: 14
about-copy-version = Copy

# @where: Toast after «Copy» on the About page
# @kind: body
# @max: 50
about-copied = The version is copied

# @where: About page, after the version: $commit is 7 hex characters or «about-build-local»; $platform like «Windows x86_64»
# @kind: value
# @max: 60
about-build = build { $commit } · { $platform }

# @where: About page: a build made on a developer's machine, not by the release pipeline
# @kind: value
# @max: 20
about-build-local = local

# @where: About page: what Znimok does, first paragraph
# @kind: body
# @max: 320
about-description = Znimok captures the screen — a region, a window, the whole screen or a long scrolling page — and records video. Arrows, text, counters, blur and the other marks stay editable after saving, text on a picture can be copied as text, and every capture goes into a local library.

# @where: About page: privacy in short, second paragraph (must match docs/privacy.md)
# @kind: body
# @max: 220
about-local = Everything stays on your computer: no account, no telemetry, no ads. Nothing leaves it without your action; the only request the program makes by itself is the update check, and it can be turned off.

# @where: About page: link button to the website
# @kind: button
# @max: 16
about-site = Website

# @where: About page: link button to the privacy policy
# @kind: button
# @max: 16
about-privacy = Privacy

# @where: About page: link button to the licence
# @kind: button
# @max: 16
about-licence = Licence

# @where: About page: copyright line; the name in the language's script (uk: Вадим Слива), «Plum» as is
# @kind: body
# @max: 70
about-copyright = © 2026 Vadym Slyva (Plum). All rights reserved.

# @where: About page: under the copyright; what the public source means
# @kind: body
# @max: 200
about-licence-note = The source code is published so that anyone can check what the program does; that is not permission to reuse it. The terms are under «Licence».

# @where: About: donation link
# @kind: button
# @max: 16
about-support = Support

# @where: About: install an update
# @kind: button
# @max: 16
about-update = Update

# @where: Banner in the library and About
# @kind: title
# @max: 40
update-available = Version { $version } is available

# @where: Banner second line; $size formatted
# @kind: body
# @max: 100
update-details = Signed, { $size }. Installs at the next launch.

# @where: Banner action
# @kind: button
# @max: 20
update-now = Update now

## Onboarding
## First run: permissions (macOS), hotkeys, library folder, agents.


# @where: macOS first run, big title
# @kind: title
# @max: 48
onb-title-mac = Three permissions — and Znimok is ready

# @where: macOS first run, under the title
# @kind: body
# @max: 200
onb-intro-mac = macOS asks for permission for everything that sees the screen. Znimok works only on your Mac and sends nothing anywhere without your action.

# @where: Windows first run, big title
# @kind: title
# @max: 48
onb-title-win = Two steps — and Znimok is ready

# @where: Windows first run, under the title
# @kind: body
# @max: 200
onb-intro-win = Znimok needs no administrator rights and sends nothing anywhere without your action. Check the hotkeys and choose where screenshots live.

# @where: Under the progress dots
# @kind: hint
# @max: 80
onb-step = Step { $n } of { $total } · you can come back here later in Settings

# @where: Step (macOS): the Screen Recording permission
# @kind: label
# @max: 40
onb-screen = Screen and system audio recording

# @where: Step state
# @kind: hint
# @max: 60
onb-screen-granted = Granted. Needed for any capture.

# @where: Step state before the grant
# @kind: hint
# @max: 80
onb-screen-needed = Needed for any capture. macOS will ask you to restart Znimok.

# @where: Step
# @kind: label
# @max: 24
onb-keys = Hotkeys

# @where: Step text (macOS); $region, $screen, $record are key combos like ⌃⇧4
# @kind: body
# @max: 300
onb-keys-mac = No permission needed. Defaults: { $region } region, { $screen } screen, { $record } recording. The usual ⌘⇧3/4/5 belong to the system: turn them off in System Settings → Keyboard → Keyboard Shortcuts, and Znimok takes them over.

# @where: Step text (Windows)
# @kind: body
# @max: 240
onb-keys-win = Defaults: { $region } region, { $screen } screen, { $record } recording. If a combination is taken by another program, pick another one here.

# @where: First-run guide, hotkeys card: the default combinations another program holds
# @kind: hint
# @max: 90
onb-keys-taken = Taken by another program: { $keys }.

# @where: First-run guide, after onb-keys-taken: the region key that works instead
# @kind: hint
# @max: 60
onb-keys-fallback = Region shots work on { $key } for now.

# @where: Step action (macOS)
# @kind: button
# @max: 24
onb-open-settings = Open settings

# @where: Step
# @kind: label
# @max: 24
onb-library = Library folder

# @where: Step text
# @kind: hint
# @max: 100
onb-library-hint = All screenshots and videos live here as files. You can pick a folder on a cloud drive.

# @where: Step (optional)
# @kind: label
# @max: 24
onb-mic = Microphone

# @where: Step text
# @kind: hint
# @max: 80
onb-mic-hint = Only for recording video with voice. You can skip it.

# @where: Step action
# @kind: button
# @max: 16
onb-allow = Allow

# @where: Step
# @kind: label
# @max: 32
onb-agents = Access for AI agents

# @where: Step text
# @kind: hint
# @max: 100
onb-agents-hint = Off. You can turn it on on the Agents page when you need it.

# @where: Footer
# @kind: button
# @max: 20
onb-skip-all = Skip all

# @where: First-run guide, bottom left: check box, ticked by default
# @kind: option
# @max: 36
onb-dont-show = Don't show next time

# @where: Settings and the first-run guide
# @kind: hint
# @max: 80
autostart-needs-approval = Allow Znimok in System Settings → General → Login Items

# @where: Settings and the first-run guide
# @kind: hint
# @max: 80
autostart-disabled-in-system = Switched off in Task Manager — the switch here turns it back on

# @where: Settings and the first-run guide
# @kind: hint
# @max: 80
onb-open-guide = First-run guide

## Crash reports
## After a crash Znimok offers its local report once at the next start. Nothing is sent automatically.


# @where: Title of the dialog shown at start after the previous run crashed
# @kind: title
# @max: 60
crash-title = Znimok closed unexpectedly last time

# @where: Text of the same dialog; $summary is the short technical reason (in English or empty)
# @kind: body
# @max: 300
crash-body = A crash report was saved on this computer: { $summary }. You can open a prefilled GitHub issue (you review it and send it yourself) or look at the report files.

# @where: Button of the crash dialog: opens github.com with the report filled in
# @kind: button
# @max: 24
crash-open-issue = Open GitHub issue

# @where: Button of the crash dialog: shows the folder with the report files
# @kind: button
# @max: 24
crash-show-folder = Show folder

## States and errors
## Empty states, errors, confirmations, toasts. Explain without blaming the user; one action each.


# @where: Error title
# @kind: title
# @max: 48
err-capture-title = Couldn't take the screenshot

# @where: Error body (macOS, no permission)
# @kind: body
# @max: 200
err-capture-mac-perm = macOS does not let Znimok see the screen. Turn on “Screen and system audio recording” in System Settings and come back.

# @where: Question title (macOS, no Screen Recording permission)
# @kind: title
# @max: 48
perm-missing-title = No screen recording permission

# @where: After the permission text: the alternative without it
# @kind: body
# @max: 200
perm-picker-hint = Or pick a window or a screen in the macOS picker now — no permission needed, but macOS puts its sharing badge on the shot.

# @where: Button: capture through the macOS content picker
# @kind: button
# @max: 28
perm-use-picker = Pick without permission

# @where: Error action
# @kind: button
# @max: 32
err-open-system-settings = Open System Settings

# @where: Error body (other causes); $reason is a short technical reason
# @kind: body
# @max: 160
err-capture-generic = The system refused the capture: { $reason }

# @where: Error title; $combo like Alt+Shift+4
# @kind: title
# @max: 60
err-key-taken-title = { $combo } is already used by another program

# @where: Error body; $action is e.g. "Region screenshot"
# @kind: body
# @max: 160
err-key-taken-body = { $action } has no hotkey for now. Choose another combination or free this one in that program.

# @where: Error action
# @kind: button
# @max: 20
err-key-choose = Choose another

# @where: Error title
# @kind: title
# @max: 48
err-disk-title = Recording stopped: the disk is full

# @where: Error body; $time like 0:41
# @kind: body
# @max: 160
err-disk-body = Everything up to this moment is saved ({ $time }). Free some space or change the library folder.

# @where: Error action
# @kind: button
# @max: 20
err-open-recording = Open recording

# @where: Error action
# @kind: button
# @max: 20
err-change-folder = Change folder

# @where: Toast when saving to the library fails
# @kind: error
# @max: 100
err-library-save = Could not save to the library. Check that the library folder is available.

# @where: Hint when the retention limit is reached
# @kind: error
# @max: 100
err-library-full = The limit is reached: the next save moves the oldest item to the trash.

# @where: Close-with-unsaved dialog; $name is the document name
# @kind: title
# @max: 60
confirm-save-title = Save changes to “{ $name }”?

# @where: Close-with-unsaved dialog
# @kind: body
# @max: 120
confirm-save-body = Your unsaved changes will be lost.

# @where: Toast with progress
# @kind: toast
# @max: 32
toast-exporting-video = Exporting video…

# @where: Toast action
# @kind: button
# @max: 16
toast-stop = Stop

## Agents
## The Agents page: connected MCP clients, models, action log, and the permission request.


# @where: Page title
# @kind: title
# @max: 24
agents-title = Agents

# @where: Yellow live indicator; $client is the agent name
# @kind: badge
# @max: 48
agents-live = { $client } is capturing the screen now

# @where: Page action
# @kind: button
# @max: 32
agents-revoke-all = Revoke all permissions

# @where: Section
# @kind: heading
# @max: 32
agents-clients = Connected clients

# @where: Section hint
# @kind: hint
# @max: 120
agents-clients-hint = Each agent gets its own key and its own permissions. Access is off until you turn it on.

# @where: Client status
# @kind: status
# @max: 16
agents-active = Active

# @where: Client status; $when is relative, e.g. "yesterday"
# @kind: status
# @max: 32
agents-last-used = Last used { $when }

# @where: Client row "…" button
# @kind: a11y
# @max: 24
agents-configure = Configure

# @where: Permission name in summaries
# @kind: label
# @max: 24
agents-scope-screen = screen capture

# @where: Permission name in summaries
# @kind: label
# @max: 24
agents-scope-library = library reading

# @where: Permission name in summaries
# @kind: label
# @max: 24
agents-scope-marks = annotations

# @where: Permission name in summaries
# @kind: label
# @max: 24
agents-scope-export = export

# @where: Summary: permission level
# @kind: label
# @max: 24
agents-grant-always = always

# @where: Summary: permission level
# @kind: label
# @max: 24
agents-grant-ask = ask every time

# @where: Summary: permission level
# @kind: label
# @max: 24
agents-grant-no = no

# @where: Dashed button under the list
# @kind: button
# @max: 40
agents-how-connect = How to connect another agent

# @where: Section
# @kind: heading
# @max: 40
agents-models = Models for Znimok features

# @where: Section hint
# @kind: hint
# @max: 140
agents-models-hint = On the device first. The cloud — only with your key and only for the features you allowed.

# @where: A permission of a connected agent
# @kind: label
# @max: 24
agents-scope-settings = settings

# @where: Switch: the local MCP server for agents on this computer
# @kind: label
# @max: 48
agents-access = Access for agents on this computer (MCP)

# @where: Under the switch: how an agent connects
# @kind: body
# @max: 160
agents-connect-hint = Claude Code: claude mcp add znimok -- znimok mcp. Claude Desktop: open the .mcpb file from the release.

# @where: The action log has no entries
# @kind: body
# @max: 48
agents-log-empty = Nothing yet

# @where: A line of the action log; $when is a date and time
# @kind: body
# @max: 80
agents-log-entry = { $when } · { $client } · { $tool }

# @where: Button: show the action log file in its folder
# @kind: button
# @max: 24
agents-log-show = Show file

# @where: Status of the cloud assistant without a key
# @kind: value
# @max: 32
agents-cloud-off = off, no key

# @where: Updates page: when the last check was; $when is a date or "never"
# @kind: value
# @max: 48
upd-last-check = Last check: { $when }

# @where: Updates page, instead of the last check when there was none
# @kind: value
# @max: 48
upd-never-checked = Not checked yet

# @where: Updates page: button to check right now
# @kind: button
# @max: 24
upd-check-now = Check now

# @where: Updates page: a check is running
# @kind: status
# @max: 32
upd-checking = Checking…

# @where: Updates page: the installer is being downloaded and verified
# @kind: status
# @max: 60
upd-downloading = Downloading and verifying the update…

# @where: Updates page (macOS): Sparkle unpacks the downloaded update
# @kind: status
# @max: 40
upd-extracting = Unpacking the update…

# @where: Updates page (macOS): Sparkle installs and relaunches the app
# @kind: status
# @max: 48
upd-installing = Installing and relaunching…

# @where: Updates page: no newer release
# @kind: status
# @max: 40
upd-up-to-date = You have the latest version

# @where: Updates page: this build has no release key yet
# @kind: status
# @max: 80
upd-not-configured = Updates are not set up in this build yet

# @where: Updates page: the check or the download failed; $reason is technical
# @kind: status
# @max: 160
upd-failed = Could not update: { $reason }

# @where: Updates page (macOS until Sparkle): open the release on GitHub
# @kind: button
# @max: 24
upd-release-page = Release page

# @where: Title of the note about the last update, shown once after it
# @kind: title
# @max: 40
upd-outcome-title = Update

# @where: The note after an update: it worked; $version is the new version
# @kind: body
# @max: 60
upd-outcome-installed = Znimok was updated to { $version }

# @where: The note after an update: the new version did not start, the previous one is back; $reason from the installer
# @kind: body
# @max: 160
upd-outcome-rolled-back = { $version } did not start ({ $reason }) — the previous version is back

# @where: The note after an update that failed; $reason from the installer
# @kind: body
# @max: 120
upd-outcome-failed = The update failed: { $reason }

# @where: Updates page: where updates come from
# @kind: body
# @max: 160
upd-channel-hint = Releases on GitHub, signed; the installer checks the signature before installing.

# @where: Model row
# @kind: label
# @max: 48
agents-ocr = Recognising text in screenshots

# @where: Model row
# @kind: label
# @max: 48
agents-redact = Finding secrets and faces to mask

# @where: Model row value
# @kind: status
# @max: 20
agents-on-device = On device

# @where: Model row
# @kind: label
# @max: 40
agents-assistant = Natural-language assistant

# @where: Model row second line; $model is a model name
# @kind: hint
# @max: 60
agents-assistant-hint = { $model } · your key · show what is sent

# @where: Row: money spent via the user's key
# @kind: label
# @max: 32
agents-spend = Spending this month

# @where: Section
# @kind: heading
# @max: 24
agents-log = Action log

# @where: Section action
# @kind: button
# @max: 12
agents-log-export = Export

# @where: Log entry
# @kind: label
# @max: 80
agents-log-window-shot = { $client } · screenshot of window “{ $title }”

# @where: Log entry
# @kind: label
# @max: 60
agents-log-window-list = { $client } · window list

# @where: Log entry; $format like PNG
# @kind: label
# @max: 80
agents-log-export-entry = { $client } · export { $format } “{ $name }”

# @where: Log entry
# @kind: label
# @max: 60
agents-log-marks =
    { $count ->
        [one] { $client } · added { $count } annotation
       *[other] { $client } · added { $count } annotations
    }

# @where: Log entry
# @kind: label
# @max: 60
agents-log-denied = { $client } · screen capture — denied

# @where: Empty state
# @kind: title
# @max: 48
agents-empty-title = No agent has connected yet

# @where: Empty state
# @kind: body
# @max: 160
agents-empty-body = Znimok can work with Claude Code and other agents. Access is off until you turn it on.

# @where: Empty state main action
# @kind: button
# @max: 24
agents-enable = Turn on access

# @where: Empty state second action
# @kind: button
# @max: 20
agents-how = How to connect

# @where: Accessible name of the permission dialog
# @kind: a11y
# @max: 40
perm-a11y = Permission request from an agent

# @where: Permission dialog title
# @kind: title
# @max: 60
perm-title = { $client } wants to capture the screen

# @where: Under the title; $key is a shortened key
# @kind: hint
# @max: 48
perm-new-client = New client · key { $key }

# @where: Permission dialog text
# @kind: body
# @max: 260
perm-body = The agent will be able to take screenshots of the screen and windows until you revoke the permission. You will see a yellow indicator while it captures, and every action goes to the log.

# @where: Checkbox
# @kind: label
# @max: 40
perm-scope-screen = Screen and window captures

# @where: Checkbox
# @kind: label
# @max: 40
perm-scope-marks = Annotations and export

# @where: Checkbox
# @kind: label
# @max: 40
perm-scope-library = Reading the library

# @where: Grant for this one request
# @kind: button
# @max: 16
perm-once = Only now

# @where: Grant until the agent disconnects
# @kind: button
# @max: 16
perm-session = This session

# @where: Grant permanently
# @kind: button
# @max: 16
perm-always = Always

# @where: Refuse
# @kind: button
# @max: 16
perm-deny = Deny

## Assistant
## Ctrl+K command bar: natural language → a plan of commands. Dialogs from the owner's decisions of 28.09.2026.


# @where: Title bar button and dialog name
# @kind: button
# @max: 16
asst-name = Assistant

# @where: Accessible name of the command field
# @kind: a11y
# @max: 16
asst-command = Command

# @where: Empty command field
# @kind: placeholder
# @max: 60
asst-placeholder = Describe what to do with the screenshot…

# @where: Plan header; $model is a model name, $sent says what went to the cloud
# @kind: status
# @max: 120
asst-plan =
    { $count ->
        [one] Plan of { $count } step · { $model } · sent: { $sent }
       *[other] Plan of { $count } steps · { $model } · sent: { $sent }
    }

# @where: Value of $sent: no pixels were sent
# @kind: label
# @max: 80
asst-sent-structure = window list and screen size, no screenshot

# @where: Plan step state
# @kind: badge
# @max: 24
asst-preview = preview

# @where: Under the plan
# @kind: hint
# @max: 100
asst-dashed-note = Dashed — what will be added. You can edit the request and try again.

# @where: Suggestions heading
# @kind: heading
# @max: 20
asst-try = Try also

# @where: Example request chip (written in the user's voice)
# @kind: option
# @max: 48
asst-example-1 = blur all e-mails and phone numbers

# @where: Example request chip
# @kind: option
# @max: 48
asst-example-2 = number the buttons left to right

# @where: Example request chip
# @kind: option
# @max: 48
asst-example-3 = crop to the settings window

# @where: Example request chip
# @kind: option
# @max: 48
asst-example-4 = find yesterday's terminal screenshot

# @where: Indicator: the request goes to the cloud
# @kind: badge
# @max: 16
asst-cloud = to the cloud

# @where: Indicator: no network
# @kind: badge
# @max: 16
asst-offline = offline

# @where: Local palette state without an API key
# @kind: hint
# @max: 100
asst-no-key = Cloud suggestions need your API key. Local commands work without it.

# @where: Action in the no-key state
# @kind: button
# @max: 24
asst-set-up-key = Add key…

# @where: Dialog before pixels leave the device
# @kind: title
# @max: 40
asst-send-title = Send the screenshot?

# @where: Dialog text
# @kind: body
# @max: 200
asst-send-body = This command needs the image. It has been masked on the device first; this is exactly what will be sent.

# @where: Checkbox
# @kind: label
# @max: 48
asst-send-dont-ask-doc = Don't ask for this document

# @where: Checkbox (global; reset in Settings)
# @kind: label
# @max: 48
asst-send-dont-ask = Don't ask again

# @where: Dialog main action
# @kind: button
# @max: 16
asst-send = Send

# @where: Dialog second action
# @kind: button
# @max: 20
asst-dont-send = Don't send

# @where: Dialog on the first cloud action
# @kind: title
# @max: 60
asst-cost-title = Cloud features cost money

# @where: Dialog text; $model name, $price is a formatted amount like $0.01
# @kind: body
# @max: 300
asst-cost-body = Requests go to { $model } with your key and are billed by Anthropic to your account. A typical request with a 1024-px screenshot costs about { $price }. Spending is shown on the Agents page.

# @where: Dialog main action
# @kind: button
# @max: 20
asst-cost-ok = Understood

# @where: Note under the bar
# @kind: hint
# @max: 160
asst-footnote = The assistant uses the same commands as MCP: every step of the plan is a command you can undo with one Ctrl+Z.

## Recording and video (v2)
## Screen recording, the video editor and video export. Shipped in v2; translated now so the layout is stable.


# @where: Recording pill button
# @kind: a11y
# @max: 16
rec-pause = Pause

# @where: Recording pill button while paused
# @kind: a11y
# @max: 16
rec-resume = Resume

# @where: Recording pill button
# @kind: a11y
# @max: 16
rec-stop = Stop

# @where: Tray / menu bar text while recording; $time like 00:12
# @kind: status
# @max: 24
rec-tray = Recording { $time }

# @where: Status while paused
# @kind: status
# @max: 24
rec-paused = Paused { $time }

# @where: Pill after recording
# @kind: title
# @max: 32
rec-saved = Video saved

# @where: Pill second line
# @kind: hint
# @max: 60
rec-saved-details = { $width } × { $height } · { $size }

# @where: DevTools log size
# @kind: badge
# @max: 32
rec-devtools-events =
    { $count ->
        [one] DevTools log: { $count } event
       *[other] DevTools log: { $count } events
    }

# @where: Pill button
# @kind: button
# @max: 16
rec-share = Share

# @where: Video editor title bar
# @kind: button
# @max: 20
vid-bug-report = Bug report

# @where: Status hint; "Space" is the key
# @kind: hint
# @max: 24
vid-hint-play = Space — play

# @where: Status hint
# @kind: hint
# @max: 24
vid-hint-frame = ← → — frame

# @where: Status hint
# @kind: hint
# @max: 24
vid-hint-cut = S — split

# @where: Transport
# @kind: a11y
# @max: 16
vid-to-start = To start

# @where: Transport
# @kind: a11y
# @max: 16
vid-frame-back = Frame back

# @where: Transport
# @kind: a11y
# @max: 16
vid-play = Play

# @where: Transport: plays the video backwards (J)
# @kind: a11y
# @max: 18
vid-play-back = Play backwards

# @where: Transport
# @kind: a11y
# @max: 16
vid-pause = Pause

# @where: Transport
# @kind: a11y
# @max: 16
vid-frame-forward = Frame forward

# @where: Transport
# @kind: a11y
# @max: 16
vid-to-end = To end

# @where: Transport
# @kind: a11y
# @max: 16
vid-speed = Speed

# @where: Transport
# @kind: a11y
# @max: 16
vid-loop = Loop

# @where: Track header
# @kind: label
# @max: 12
vid-track-system = System

# @where: Track header
# @kind: label
# @max: 12
vid-track-mic = Microphone

# @where: Button: take the current frame as a screenshot
# @kind: button
# @max: 24
vid-frame-as-shot = Frame as screenshot

# @where: Timeline lane label (short)
# @kind: label
# @max: 8
vid-lane-video = video

# @where: Timeline lane label (short)
# @kind: label
# @max: 8
vid-lane-sound = sound

# @where: Timeline lane label (short)
# @kind: label
# @max: 8
vid-lane-mic = mic

# @where: Timeline lane label (short)
# @kind: label
# @max: 8
vid-lane-marks = marks

# @where: Clip tab section
# @kind: label
# @max: 16
vid-trim = Trim

# @where: Trim field
# @kind: label
# @max: 8
vid-from = From

# @where: Trim field
# @kind: label
# @max: 8
vid-to = To

# @where: Under the trim; $seconds formatted like 4.9
# @kind: hint
# @max: 100
vid-cuts =
    { $count ->
        [one] { $count } gap cut · { $seconds } s. Delete cuts the selection.
       *[other] { $count } gaps cut · { $seconds } s. Delete cuts the selection.
    }

# @where: Clip tab section
# @kind: label
# @max: 12
vid-sound = Sound

# @where: Under the sound section
# @kind: hint
# @max: 80
vid-tracks-note = Tracks are separate; export mixes them into one.

# @where: Clip tab section
# @kind: label
# @max: 24
vid-size-frame = Size and crop

# @where: Events count
# @kind: status
# @max: 16
vid-events =
    { $count ->
        [one] { $count } event
       *[other] { $count } events
    }

# @where: Status bar: current frame number
# @kind: status
# @max: 16
vid-frame-n = frame { $n }

# @where: Status bar
# @kind: status
# @max: 40
vid-marks-here =
    { $count ->
        [one] { $count } annotation on this frame
       *[other] { $count } annotations on this frame
    }

# @where: Status bar
# @kind: status
# @max: 32
vid-cursor-recorded = Cursor and clicks recorded

# @where: Status bar; $fps is a number
# @kind: status
# @max: 16
vid-fps = { $fps } fps

# @where: Video export dialog
# @kind: title
# @max: 24
vexp-title = Export video

# @where: Field
# @kind: label
# @max: 16
vexp-fps = Frames/s

# @where: Field
# @kind: label
# @max: 12
vexp-width = Width

# @where: Field (GIF)
# @kind: label
# @max: 12
vexp-colours = Colours

# @where: Checkbox (GIF)
# @kind: label
# @max: 16
vexp-dither = Dithering

# @where: Checkbox (GIF)
# @kind: label
# @max: 16
vexp-loop = Loop

# @where: Before the estimate
# @kind: label
# @max: 12
vexp-estimate = Estimate:

# @where: Warning for large GIFs
# @kind: hint
# @max: 140
vexp-gif-warning = GIFs over 25 MB load poorly in chats. MP4 or WebP suits such clips better.

## For developers
## A temporary settings page for testing (owner, 29.09).


# @where: Settings → For developers
# @kind: button
# @max: 60
dev-page = For developers

# @where: Settings → For developers: the note at the top of the page
# @kind: body
# @max: 120
dev-intro = A temporary page for testing: reset state and show what normally appears only once.

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-onboarding = Show the first-run guide again

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-pill = Show the card after a capture

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-crash = Show the crash-report question

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-open-settings = Open the settings folder

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-open-logs = Open the logs folder

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-empty-trash = Empty the library trash

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-reset = Reset all settings

# @where: Settings → For developers
# @kind: button
# @max: 60
dev-done = Done

## Library trash and picking cards (ZK-175, ZK-176)


# @where: Library header: the switch to the library's trash
# @kind: tab
# @max: 16
lib-trash-tab = Trash

# @where: Library → Trash header: delete everything in the trash for good
# @kind: button
# @max: 24
trash-destroy-all = Destroy all

# @where: Library → Trash: card button and the picked bar; back into the library
# @kind: button
# @max: 16
trash-restore = Restore

# @where: Library → Trash: card button, the picked bar and the question's button; delete for good
# @kind: button
# @max: 16
trash-destroy = Destroy

# @where: Library → Trash, nothing in it
# @kind: title
# @max: 40
trash-empty-title = The trash is empty

# @where: Library → Trash, nothing in it; $days is the setting
# @kind: body
# @max: 160
trash-empty-body = Deleted documents stay here for { $days } days, then go for good. The term is in Settings → Library.

# @where: Library: the bar over picked cards; $count cards
# @kind: label
# @max: 24
lib-picked = Selected: { $count }

# @where: Library: the bar over picked cards, the close button
# @kind: tooltip
# @max: 32
lib-pick-none = Clear selection

# @where: Settings → Library: the days a deleted document stays in the trash
# @kind: label
# @max: 60
libset-trash-days = Keep deleted documents in the trash, days

# @where: Question before deleting from the trash for good
# @kind: title
# @max: 40
trash-destroy-title = Destroy for good?

# @where: Toast after several documents went to the trash, with Undo
# @kind: toast
# @max: 48
lib-trashed-many-toast =
    { $count ->
        [one] { $count } document moved to the trash
       *[other] { $count } documents moved to the trash
    }

# @where: Toast after documents came back from the trash
# @kind: toast
# @max: 48
trash-restored-toast =
    { $count ->
        [one] { $count } document restored
       *[other] { $count } documents restored
    }

# @where: Body of the question before deleting from the trash; $count documents
# @kind: body
# @max: 160
trash-destroy-body =
    { $count ->
        [one] { $count } document will be deleted for good. This cannot be undone.
       *[other] { $count } documents will be deleted for good. This cannot be undone.
    }

# @where: Library → Trash: a card's line; $date when it went in, $days until it goes for good
# @kind: label
# @max: 48
trash-card-meta =
    { $days ->
        [one] deleted { $date } · goes in { $days } day
       *[other] deleted { $date } · goes in { $days } days
    }

## Library groups, pins and keys (ZK-177, ZK-178, ZK-179)


# @where: Library card button: pin the document
# @kind: tooltip
# @max: 60
lib-pin = Pin — the library limit never removes it

# @where: Library: the bar over picked cards, pin them
# @kind: button
# @max: 16
lib-pin-short = Pin

# @where: Library: unpin (card button and the bar over picked cards)
# @kind: button
# @max: 16
lib-unpin = Unpin

# @where: Library grid: title of the pinned documents' group
# @kind: title
# @max: 24
lib-group-pinned = Pinned

# @where: Library grid: group title
# @kind: title
# @max: 24
lib-group-today = Today

# @where: Library grid: group title
# @kind: title
# @max: 24
lib-group-yesterday = Yesterday

# @where: Library grid: group title, earlier this week
# @kind: title
# @max: 24
lib-group-week = This week

# @where: Library grid: group title, earlier this month
# @kind: title
# @max: 24
lib-group-month = This month

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-1 = January

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-2 = February

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-3 = March

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-4 = April

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-5 = May

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-6 = June

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-7 = July

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-8 = August

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-9 = September

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-10 = October

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-11 = November

# @where: Library grid: group title of an older month (the year follows when it is not this year)
# @kind: title
# @max: 16
month-12 = December

## Video mode of the editor (ZK-181)


# @where: Inspector tab for a video document, in place of «Image»
# @kind: tab
# @max: 12
insp-tab-video = Video

# @where: Video transport: the start of the kept video is the current frame (I)
# @kind: button
# @max: 16
vid-in-here = Start here

# @where: Video transport: the end of the kept video is the current frame (O)
# @kind: button
# @max: 16
vid-out-here = End here

# @where: Video transport: split the strip at the current frame (S)
# @kind: button
# @max: 16
vid-split-here = Split here

# @where: Video timeline: cut the selected piece (Del); also the row «Cut» in the Trim section
# @kind: button
# @max: 16
vid-cut = Cut

# @where: Video timeline: cut everything but the selected piece (Shift+Del)
# @kind: button
# @max: 24
vid-keep-only = Keep only this

# @where: Video timeline: a cut-out piece comes back
# @kind: button
# @max: 16
vid-restore = Restore

# @where: Video, Trim section: forget every trim and cut
# @kind: button
# @max: 24
vid-reset-trim = Reset trimming

# @where: Video, Trim section: how long the video is after the edits
# @kind: label
# @max: 16
vid-left = Left

# @where: Video, Trim section: a note under the numbers
# @kind: body
# @max: 160
vid-trim-note = The recording is never changed: what is cut can come back, and the file is rewritten only on export.

# @where: Video, Trim section: how the strip is used
# @kind: body
# @max: 120
vid-tl-hint = Drag on the strip to pick a piece · Del cuts it · Shift+Del keeps only it · S splits

# @where: Video, Sound section: no track goes into the export
# @kind: button
# @max: 16
vid-no-sound = No sound

# @where: Video, Sound section: both tracks go into the export
# @kind: button
# @max: 12
vid-sound-both = Both

# @where: Video transport: the whole timeline in view
# @kind: button
# @max: 12
vid-fit = Fit

# @where: Video, Trim section: the player could not start on this machine
# @kind: body
# @max: 160
vid-poster-note = The video could not be played here: the first frame stands for it; trimming works.

# @where: Name of the screenshot made from a video frame; $name the video, $n the frame
# @kind: label
# @max: 60
vid-frame-doc-name = { $name } — frame { $n }

# @where: Video, Trim section: how many pieces are cut out and their length; $count, $seconds
# @kind: label
# @max: 40
vid-cut-count =
    { $count ->
        [one] { $count } piece · { $seconds } s
       *[other] { $count } pieces · { $seconds } s
    }

# @where: Video transport: the timeline shows more time
# @kind: tooltip
# @max: 24
vid-zoom-out = Timeline: zoom out

# @where: Video transport: the timeline shows less time, larger
# @kind: tooltip
# @max: 24
vid-zoom-in = Timeline: zoom in

# @where: Video, Sound section: put a muted track back into the export
# @kind: button
# @max: 12
vid-track-on = Turn on

# @where: Video, Sound section: leave a track out of the export
# @kind: button
# @max: 12
vid-track-off = Turn off

## Recording in the app (ZK-180, ZK-91)


# @where: Settings → Hotkeys: the row of the recording hotkey
# @kind: label
# @max: 40
keys-video = Start / stop video recording

# @where: Tray menu while recording, followed by the time
# @kind: menu
# @max: 24
tray-stop-record = Stop recording

# @where: Settings → Recording: how a recording starts and ends
# @kind: body
# @max: 240
rec-how = The recording hotkey or «Record video» in the tray opens the same overlay as for screenshots: drag — a region, click — a window, Space — the whole screen. The same key or «Stop» ends it; the video goes to the library.

# @where: A recording could not start; $reason from the system
# @kind: toast
# @max: 160
rec-error-start = Could not start recording: { $reason }

# @where: A finished recording could not be saved to the library; $reason
# @kind: toast
# @max: 160
rec-error-save = The recording could not be saved: { $reason }

# @where: Settings → Recording, under «Cursor and click highlight»
# @kind: hint
# @max: 220
rec-cursor-hint = The pointer is drawn into the video, a click is a spreading ring and a held button a steady one; clicks on the recording bar are not recorded. The clicks are also kept as a log in the document.

# @where: Settings → Recording, under the sound choice
# @kind: hint
# @max: 220
rec-sound-tracks = Each source is a track of its own: in the editor it can be muted or made quieter. The system sound is what this computer plays; the microphone needs Windows' permission (Privacy → Microphone).

# @where: Toast after a recording: Windows privacy settings deny the microphone
# @kind: status
# @max: 200
rec-warn-mic-denied = Recorded without the microphone: Windows does not allow it (Settings → Privacy → Microphone → desktop apps).

# @where: Toast after a recording: another program holds the sound device exclusively
# @kind: status
# @max: 200
rec-warn-audio-busy = Recorded without some sound: another program holds the sound device.

# @where: Toast after a recording: a sound source could not be opened (no device, or the encoder took no audio)
# @kind: status
# @max: 200
rec-warn-audio-none = Recorded without some sound: the sound device could not be opened.

# @where: Name of a new recording in the library; $date, $time
# @kind: label
# @max: 40
rec-doc-name = Recording { $date } { $time }

# @where: Recording is not available on this system yet (macOS until the next update)
# @kind: toast
# @max: 120
rec-not-here = Recording on this system comes with the next update.

# @where: A recording ended without a single frame (a window that never changed)
# @kind: toast
# @max: 120
rec-nothing = Nothing was recorded: the window did not change while it was being recorded.

## The «Відео» tab (ZK-188)


# @where: Crop panel: free proportions (the other presets are ratios like 16:9)
# @kind: button
# @max: 10
crop-aspect-free = Free

# @where: Video tab: section title, the playback speed
# @kind: label
# @max: 24
vid-speed-title = Speed

# @where: Video tab: under the speed buttons
# @kind: body
# @max: 120
vid-speed-note = Playback in the editor only; the export keeps the real speed.

# @where: Video tab: section title, the width and height of the exported video
# @kind: label
# @max: 30
vid-out-title = Size on export

# @where: Video tab: under the size on export
# @kind: body
# @max: 160
vid-out-note = The frame is scaled on export; the recording stays as it is. Sides are even (the encoder needs it).

## The browser log (ZK-97)


# @where: Settings → Recording: section title, the browser log
# @kind: label
# @max: 24
rec-browser = Browser

# @where: Settings → Recording: switch, the DevTools log of a recording
# @kind: label
# @max: 70
rec-devlog = Write the browser's DevTools log with a recording

# @where: Settings → Recording: switch, the extension may start recordings
# @kind: label
# @max: 70
rec-ext-control = Let the extension start a recording of its window

# @where: Settings → Recording: the extension is connected; $count browsers
# @kind: body
# @max: 80
rec-browsers-on =
    { $count ->
        [one] The extension is connected (one browser).
       *[other] The extension is connected ({ $count } browsers).
    }

# @where: Settings → Recording: no browser with the extension is connected
# @kind: body
# @max: 90
rec-browsers-off = No browser with the Znimok extension is connected.

# @where: Settings → Recording: what the browser log writes and how to install the extension
# @kind: body
# @max: 500
rec-devlog-hint = The Znimok extension for Chrome and Edge writes the page's console, errors, network (headers, request and response bodies) and navigations in sync with the video — everything the DevTools panels show, for debugging; sensitive parts can be hidden on export. Install it from the Chrome Web Store (the button below).

# @where: Settings → Recording: the button under the extension's hint; opens its Chrome Web Store page
# @kind: button
# @max: 24
rec-devlog-store = Chrome Web Store

## Video export (ZK-190)


# @where: The header button and menu of a video: the MP4 file to the clipboard
# @kind: button
# @max: 18
vid-copy-mp4 = Copy MP4

# @where: Share menu of a video: a GIF file to the clipboard
# @kind: menu
# @max: 24
vid-copy-gif = Copy GIF

# @where: Share menu of a video: the frame shown with its marks, as a picture
# @kind: menu
# @max: 24
vid-copy-frame = Copy frame

# @where: Share menu of a video: the exported MP4 as a new library document
# @kind: menu
# @max: 30
vid-save-library = Save to the library

# @where: Video export sheet: the MP4 card title
# @kind: label
# @max: 10
vexp-mp4 = MP4

# @where: Video export sheet: the MP4 card
# @kind: body
# @max: 90
vexp-mp4-desc = Plays in any player and chat; sound mixed into one track.

# @where: Video export sheet: the GIF card title
# @kind: label
# @max: 10
vexp-gif = GIF

# @where: Video export sheet: the GIF card, its width and rate; $w, $fps
# @kind: label
# @max: 24
vexp-gif-sub = { $w } px · { $fps } fps

# @where: Video export sheet: the GIF card
# @kind: body
# @max: 90
vexp-gif-desc = Plays anywhere without a player; no sound, 255 colours.

# @where: Video export sheet: the HTML card title
# @kind: label
# @max: 10
vexp-html = HTML

# @where: Video export sheet: the HTML card, under its title
# @kind: label
# @max: 20
vexp-html-sub = one page

# @where: Video export sheet: the HTML card
# @kind: body
# @max: 90
vexp-html-desc = The video with its marks as a live layer; opens in any browser.

# @where: Export sheet, the HTML card's subtitle when the recording has the browser's log (ZK-226)
# @kind: label
# @max: 28
vexp-html-sub-log = one page with the log

# @where: Export sheet, the HTML card's description when the recording has the browser's log
# @kind: body
# @max: 90
vexp-html-desc-log = The video, its marks and the browser's DevTools log in one page; opens in any browser.

# @where: Export sheet, under the HTML card when the recording has the browser's log
# @kind: body
# @max: 160
vexp-html-log-note = The page carries the browser's DevTools log beside the video, as the report does; sensitive values are hidden as set below.

# @where: Video export sheet: the frame-as-screenshot card
# @kind: body
# @max: 90
vexp-frame-desc = The current frame with its marks, as a new screenshot.

# @where: Video export sheet: the card of the report with the browser log (not yet)
# @kind: label
# @max: 40
vexp-report = Report with the DevTools log

# @where: Video export sheet: a card not available yet
# @kind: label
# @max: 20
vexp-soon = soon

# @where: Video export sheet: the report card (not yet)
# @kind: body
# @max: 90
vexp-report-desc = The video, its marks and the browser's DevTools log in one page, for a bug report.

# @where: Video export sheet, GIF: the size limit label
# @kind: label
# @max: 24
vexp-limit = Not more than

# @where: Video export sheet, GIF: no size limit
# @kind: label
# @max: 16
vexp-limit-off = no limit

# @where: Video export sheet, GIF: under the size limit
# @kind: body
# @max: 140
vexp-limit-note = Over the limit, Znimok lowers the frame rate, then the width, and says what it changed.

# @where: Video export sheet: what the MP4 export does
# @kind: body
# @max: 200
vexp-mp4-note = H.264 with the hardware encoder; the sound tracks that are on are mixed into one; what is cut is left out with short fades at the joints.

# @where: Video export sheet: what the HTML export does
# @kind: body
# @max: 200
vexp-html-note = The video and its marks in one file: the marks stay a layer over the video, shown in their time; a hide and a marker go into the video itself.

# @where: Video export sheet: what the frame card does
# @kind: body
# @max: 160
vexp-frame-note = The frame on the canvas, full size, with the marks shown on it — a new screenshot in a window of its own.

# @where: Video export sheet: the clipboard gets a file
# @kind: body
# @max: 120
vexp-clipboard-note = The clipboard gets the file: paste it into a chat or a folder.

# @where: Video export sheet: progress; $pct
# @kind: label
# @max: 30
vexp-working = Exporting… { $pct } %

# @where: Toast when a quick export starts; $format
# @kind: toast
# @max: 60
vexp-working-toast = Making the { $format }…

# @where: Toast: an exported video file is on the clipboard; $format
# @kind: toast
# @max: 90
vexp-copied = { $format } copied — paste it into a chat or a folder

# @where: Toast: the exported MP4 is a new library document
# @kind: toast
# @max: 60
vexp-library-done = The MP4 is in the library

# @where: Toast addition: what the GIF limit changed; $w, $fps
# @kind: toast
# @max: 60
vexp-lowered = to fit: { $w } px, { $fps } fps

# @where: Toast: the export was cancelled
# @kind: toast
# @max: 40
vexp-cancelled = Export cancelled

# @where: Toast: the export failed; $reason
# @kind: toast
# @max: 120
vexp-error = Export failed: { $reason }

# @where: Video export sheet header: kept length, size, what applies; $time, $w, $h, $what
# @kind: label
# @max: 90
vexp-subtitle = { $time } after trimming · { $w } × { $h } { $what }

# @where: Video export sheet header: marks and the frame are applied
# @kind: label
# @max: 40
vexp-applied-both = · marks and frame applied

# @where: Video export sheet header: marks are applied
# @kind: label
# @max: 30
vexp-applied-marks = · marks applied

# @where: Video export sheet header: the frame (crop / size) is applied
# @kind: label
# @max: 30
vexp-applied-frame = · frame applied

## The DevTools log panel (ZK-191)


# @where: Video timeline: the DevTools log lane title (short, lower case like the other lanes)
# @kind: label
# @max: 10
vid-lane-log = log

# @where: Video timeline: tooltip, opens the DevTools log panel
# @kind: label
# @max: 40
devp-show = Show the log

# @where: Video timeline: tooltip, hides the DevTools log panel
# @kind: label
# @max: 40
devp-hide = Hide the log

# @where: Video transport: button, jumps to the next error of the browser log
# @kind: label
# @max: 40
devp-next-error = To the next error

# @where: DevTools log panel: search field placeholder
# @kind: label
# @max: 30
devp-search = Search the log

# @where: DevTools log panel: no row matches the search and filter
# @kind: body
# @max: 60
devp-empty = Nothing matches.

# @where: DevTools log panel: button, saves a response body to a file
# @kind: label
# @max: 24
devp-save = Save as…

# @where: Toast: a response body was saved; $name the file name
# @kind: body
# @max: 80
devp-saved = Saved: { $name }

# @where: Toast: a response body could not be saved
# @kind: body
# @max: 80
devp-save-failed = Could not save the file.

# @where: DevTools log panel: tab of a request (as in Chrome DevTools)
# @kind: label
# @max: 14
devp-tab-headers = Headers

# @where: DevTools log panel: tab, the request body (as in Chrome DevTools)
# @kind: label
# @max: 14
devp-tab-payload = Payload

# @where: DevTools log panel: tab, the response formatted (as in Chrome DevTools)
# @kind: label
# @max: 14
devp-tab-preview = Preview

# @where: DevTools log panel: tab, the raw response (as in Chrome DevTools)
# @kind: label
# @max: 14
devp-tab-response = Response

# @where: DevTools log panel: tab, the request phases (as in Chrome DevTools)
# @kind: label
# @max: 14
devp-tab-timing = Timing

# @where: DevTools log panel: filter chip, every event (a count follows)
# @kind: label
# @max: 16
devp-chip-all = All

# @where: DevTools log panel: filter chip, errors (a count follows)
# @kind: label
# @max: 16
devp-chip-errors = Errors

# @where: DevTools log panel: filter chip, warnings (a count follows)
# @kind: label
# @max: 16
devp-chip-warnings = Warnings

# @where: DevTools log panel: filter chip, requests and WebSocket frames (a count follows)
# @kind: label
# @max: 16
devp-chip-network = Network

# @where: DevTools log panel: filter chip, console messages and exceptions (a count follows)
# @kind: label
# @max: 16
devp-chip-console = Console

# @where: DevTools log panel: filter chip, page navigations (a count follows)
# @kind: label
# @max: 16
devp-chip-nav = Navigation

# @where: DevTools log panel, Headers tab: section title (as «General» in Chrome DevTools)
# @kind: label
# @max: 30
devp-general = General

# @where: DevTools log panel, Headers tab: section title
# @kind: label
# @max: 30
devp-res-headers = Response headers

# @where: DevTools log panel, Headers tab: section title
# @kind: label
# @max: 30
devp-req-headers = Request headers

# @where: DevTools log panel, Headers tab: field, the request address
# @kind: label
# @max: 20
devp-h-url = URL

# @where: DevTools log panel, Headers tab: field, GET / POST…
# @kind: label
# @max: 20
devp-h-method = Method

# @where: DevTools log panel, Headers tab: field, the HTTP status
# @kind: label
# @max: 20
devp-h-status = Status

# @where: DevTools log panel, Headers tab: field, the server IP address
# @kind: label
# @max: 20
devp-h-remote = Remote address

# @where: DevTools log panel, Headers tab: field, h2 / http/1.1
# @kind: label
# @max: 20
devp-h-protocol = Protocol

# @where: DevTools log panel, Headers tab: field, what started the request
# @kind: label
# @max: 20
devp-h-initiator = Initiator

# @where: DevTools log panel, Headers tab: field, served from the cache
# @kind: label
# @max: 20
devp-h-cache = From cache

# @where: DevTools log panel, Headers tab: field, why the request failed
# @kind: label
# @max: 20
devp-h-error = Error

# @where: DevTools log panel: title above a call stack
# @kind: label
# @max: 20
devp-stack = Stack

# @where: DevTools log panel: under a long text, only its start is shown
# @kind: body
# @max: 120
devp-cut = Only the first 256 KB are shown; «Save as…» keeps it all.

# @where: DevTools log panel: a binary response; $size its size
# @kind: body
# @max: 120
devp-binary = Binary data, { $size }. «Save as…» keeps it.

# @where: DevTools log panel: an event without details
# @kind: body
# @max: 40
devp-nothing = No data.

# @where: DevTools log panel, Timing tab: phase, waiting before the request (as «Queueing» in DevTools)
# @kind: label
# @max: 20
devp-t-queue = Queueing

# @where: DevTools log panel, Timing tab: phase, the DNS lookup
# @kind: label
# @max: 20
devp-t-dns = DNS lookup

# @where: DevTools log panel, Timing tab: phase, the connection
# @kind: label
# @max: 20
devp-t-connect = Connection

# @where: DevTools log panel, Timing tab: phase, the TLS handshake
# @kind: label
# @max: 20
devp-t-tls = TLS

# @where: DevTools log panel, Timing tab: phase, sending the request
# @kind: label
# @max: 20
devp-t-send = Request sent

# @where: DevTools log panel, Timing tab: phase, waiting for the server (TTFB)
# @kind: label
# @max: 20
devp-t-wait = Waiting (TTFB)

# @where: DevTools log panel, Timing tab: phase, receiving the response
# @kind: label
# @max: 20
devp-t-download = Download

# @where: DevTools log panel, Timing tab: the whole request
# @kind: label
# @max: 20
devp-t-total = Total

## The developer report (ZK-98)


# @where: Export sheet, the developer report: what the two forms are
# @kind: body
# @max: 240
vexp-report-note = One page up to 100 MB opens in any browser; a .zreport keeps the video beside the page and the log and dataLayer as JSON, and opens in Znimok as a recording.

# @where: Export sheet, the developer report: form option, one HTML page
# @kind: label
# @max: 22
vexp-report-html = One page (HTML)

# @where: Export sheet, the developer report: form option, a ZIP archive
# @kind: label
# @max: 22
vexp-report-zip = Archive (.zreport)

# @where: Export sheet, the developer report: the estimate is over 100 MB as one page
# @kind: body
# @max: 100
vexp-report-big = Over 100 MB as one page — choose the .zreport.

# @where: Export sheet, the developer report: the recording has no browser log
# @kind: body
# @max: 120
vexp-no-log = This recording has no browser log: the report has the video and its marks.

# @where: Export sheet, the developer report: switch, hide sensitive values of the log
# @kind: label
# @max: 40
vexp-hide = Hide sensitive values

# @where: Export sheet, the developer report: Settings say always hide
# @kind: body
# @max: 100
vexp-hide-always = Sensitive values are always hidden (Settings → Recording).

# @where: Export sheet, the developer report: Settings say never hide
# @kind: body
# @max: 100
vexp-hide-never = Sensitive values are not hidden (Settings → Recording).

# @where: Export sheet, the developer report: how many values will be hidden; $count
# @kind: body
# @max: 200
vexp-hide-count =
    { $count ->
        [one] One value will be hidden: keys from the list in Settings and secrets by their look.
       *[other] { $count } values will be hidden: keys from the list in Settings and secrets by their look.
    }

# @where: Export sheet, the developer report: nothing sensitive was found in the log
# @kind: body
# @max: 60
vexp-hide-none = Nothing sensitive found in the log.

# @where: Toast: the report is over 100 MB as one page
# @kind: body
# @max: 100
vexp-report-too-big = The report is over 100 MB as one page — export it as a .zreport.

# @where: Developer report page: header fact, when it was recorded
# @kind: label
# @max: 20
report-recorded = Recorded

# @where: Developer report page: header fact, the video length
# @kind: label
# @max: 20
report-length = Length

# @where: Developer report page: header fact, the video size in pixels
# @kind: label
# @max: 20
report-size = Size

# @where: Developer report page: header fact, the browser
# @kind: label
# @max: 20
report-browser = Browser

# @where: Developer report page: header fact, the page address the log starts on
# @kind: label
# @max: 20
report-page = Page

# @where: Developer report page: how many values of the log were hidden; $n a number
# @kind: body
# @max: 60
report-masked = { $n } values hidden

# @where: Developer report page: footer; $version the app version
# @kind: body
# @max: 60
report-foot = Made with Znimok { $version }

# @where: Developer report page: the recording has no browser log
# @kind: body
# @max: 80
report-no-log = This recording has no browser log.

# @where: Report page (the browser's viewer): the chip that filters the clicks of the recording
# @kind: label
# @max: 12
report-chip-clicks = Clicks

# @where: Report page: the kind column of a click row
# @kind: label
# @max: 12
report-click = Click

# @where: Report page: the keyboard keys, at the bottom right
# @kind: body
# @max: 120
report-keys = ↑ ↓ — events · E — next error · Space — play / pause · ← → — ±2 s · / — search · Esc — close

# @where: Toast: a .zreport could not be opened; $reason why
# @kind: body
# @max: 120
zreport-error = Cannot open the report: { $reason }

# @where: Settings → Recording: section, hiding values in a developer report
# @kind: label
# @max: 50
rec-hide-title = Hide sensitive values in a report

# @where: Settings → Recording: option, the export sheet asks
# @kind: label
# @max: 14
rec-hide-ask = Ask

# @where: Settings → Recording: option, always hide
# @kind: label
# @max: 14
rec-hide-always = Always

# @where: Settings → Recording: option, never hide
# @kind: label
# @max: 14
rec-hide-never = Never

# @where: Settings → Recording: label of the field with the keys to hide
# @kind: label
# @max: 30
rec-hide-keys = Keys to hide

# @where: Settings → Recording: what the keys are and what else is hidden
# @kind: body
# @max: 300
rec-hide-hint = Comma-separated: headers, JSON keys, form fields and dataLayer keys whose values become •••. Secrets such as tokens, e-mails and card numbers are found by their look as well. The log stays whole in your files.

## dataLayer in the DevTools log (ZK-195)


# @where: DevTools log panel: filter chip, values pushed into dataLayer by GTM / gtag (a count follows)
# @kind: label
# @max: 16
devp-chip-datalayer = dataLayer

# @where: DevTools log panel: a dataLayer value that was already there when the recording started
# @kind: body
# @max: 120
devp-dl-pre = Already in dataLayer when the recording started.

# @where: DevTools log panel: a dataLayer value from a frame inside the page
# @kind: body
# @max: 80
devp-dl-frame = From a frame inside the page.

## The search by text on screenshots (ZK-186)


# @where: Settings → Library: switch, search the text on screenshots too
# @kind: label
# @max: 50
libset-search-text = Search the text on screenshots too

# @where: Settings → Library: what the search by text does and keeps
# @kind: body
# @max: 300
libset-search-text-hint = Znimok reads the text on your screenshots on this computer, slowly in the background, and keeps it only in the library's local index — not in the files and nowhere online. Turned off, what was read is forgotten.

# @where: Settings → Library: progress of reading; $done of $count screenshots
# @kind: body
# @max: 80
libset-text-reading = Reading the text: { $done } of { $count }…

# @where: Settings → Library: all screenshots are read; $count of them
# @kind: body
# @max: 80
libset-text-ready =
    { $count ->
        [one] The text of one screenshot is searchable.
       *[other] The text of { $count } screenshots is searchable.
    }

# @where: Report page: the first tab of an event's details (not a network request)
# @kind: label
# @max: 16
report-tab-details = Details

# @where: Report page: the details pane while no event is chosen
# @kind: body
# @max: 60
report-pick = Choose an event to see its details

# @where: Report page: the button that copies what the details show
# @kind: button
# @max: 14
report-copy = Copy

# @where: Report page, a request's details: the resource type and its MIME type
# @kind: label
# @max: 16
report-h-type = Type

# @where: Report page, a request's details: bytes transferred
# @kind: label
# @max: 16
report-h-size = Size

# @where: Report page, a console message's details: the script and line it came from
# @kind: label
# @max: 16
report-h-source = Source

# @where: Report page, a tab's details: the page title
# @kind: label
# @max: 16
report-h-title = Title

# @where: Developer report page: header fact, who made the recording (name, contact)
# @kind: label
# @max: 20
report-author = Recorded by

# @where: Settings → Recording: heading of the fields that sign a developer report
# @kind: label
# @max: 40
rec-sign-title = Report signature

# @where: Settings → Recording, report signature: placeholder of the name field
# @kind: placeholder
# @max: 30
rec-sign-name = Your name

# @where: Settings → Recording, report signature: placeholder of the contact field
# @kind: placeholder
# @max: 40
rec-sign-contact = E-mail or another contact

# @where: Settings → Recording, report signature: placeholder of the rights notice field
# @kind: placeholder
# @max: 70
rec-sign-rights = Rights notice, e.g. © 2026 Company. Confidential.

# @where: Settings → Recording, report signature: what the fields are for
# @kind: body
# @max: 200
rec-sign-hint = A report page says who recorded it at the top and whose its content is at the bottom. Leave the fields empty for an unsigned report.

# @where: Video export sheet, a page with the browser log: label of the page's language choice
# @kind: label
# @max: 24
vexp-lang = Page language

# @where: Video export sheet, a page with the browser log: switch, add the signature from the settings
# @kind: label
# @max: 50
vexp-sign = Sign: who recorded it, the rights notice

# @where: Video export sheet, a page with the browser log: no signature is set
# @kind: body
# @max: 100
vexp-sign-none = Your name and a rights notice can be added in Settings → Recording.

# @where: Title of the question when an AI agent wants access; $client is the name the agent reports
# @kind: title
# @max: 60
agents-ask-title = «{ $client }» asks for access

# @where: Body of the access question; $what is one of agents-ask-capture…, $tool the tool's name
# @kind: body
# @max: 260
agents-ask-body = An AI agent that calls itself «{ $client }» wants to { $what }. It asked through «{ $tool }». The name is what the program says about itself — allow only what you started yourself.

# @where: Inside agents-ask-body after «wants to»: screenshots
# @kind: body
# @max: 80
agents-ask-capture = take screenshots and see the list of open windows

# @where: Inside agents-ask-body after «wants to»: reading the library
# @kind: body
# @max: 80
agents-ask-library-read = read the documents of your library and export copies

# @where: Inside agents-ask-body after «wants to»: changing the library
# @kind: body
# @max: 80
agents-ask-library-write = change documents of your library and add new ones

# @where: Inside agents-ask-body after «wants to»: settings
# @kind: body
# @max: 80
agents-ask-settings = read and change Znimok's settings

# @where: Inside agents-ask-body after «wants to»: an access this version does not know
# @kind: body
# @max: 80
agents-ask-other = use a part of Znimok this version does not know

# @where: Access question: allow this one call
# @kind: button
# @max: 20
agents-ask-once = This time

# @where: Access question: allow until the agent's session ends
# @kind: button
# @max: 20
agents-ask-session = This session

# @where: Access question: allow from now on
# @kind: button
# @max: 20
agents-ask-always = Always

# @where: Access question: refuse
# @kind: button
# @max: 20
agents-ask-deny = Deny

# @where: Title of the question when an AI agent wants to delete a document for good; $name is the document
# @kind: title
# @max: 80
agents-confirm-delete-title = Delete «{ $name }» for good?

# @where: Body of that question; $client is the name the agent reports
# @kind: body
# @max: 160
agents-confirm-delete-body = The AI agent «{ $client }» asks for it. This cannot be undone.

# @where: Inside agents-ask-body after «wants to»: screen recording
# @kind: body
# @max: 80
agents-ask-record = record the screen as video, also while you are away

# @where: Inside agents-ask-body after «wants to»: sound of a recording
# @kind: body
# @max: 80
agents-ask-record-audio = record the computer's sound and the microphone

# @where: Settings → Agents: a permission in a client's list
# @kind: label
# @max: 24
agents-scope-record = screen recording

# @where: Settings → Agents: a permission in a client's list
# @kind: label
# @max: 24
agents-scope-sound = sound and microphone
