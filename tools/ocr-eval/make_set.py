#!/usr/bin/env python3
"""Builds the OCR evaluation set (ZK-120): screenshot-like pictures of Ukrainian (and mixed)
text with the exact text next to each. Rendered with the Windows fonts of this machine — UI
text as it appears on screen — so the set is committed, not regenerated in CI.

    python tools/ocr-eval/make_set.py            # writes tools/ocr-eval/set/*.png + *.txt

No private data: every text below is made up.
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

OUT = Path(__file__).resolve().parent / "set"
FONTS = Path("C:/Windows/Fonts")

# (file, name)
FACES = [
    ("segoeui.ttf", "segoe"),
    ("arial.ttf", "arial"),
    ("calibri.ttf", "calibri"),
    ("consola.ttf", "consolas"),
    ("times.ttf", "times"),
]

LINES = [
    "Зберегти зміни перед закриттям?",
    "Налаштування → Конфіденційність і захист",
    "Їжак ґречно з’їв яєчню й пішов у поле.",
    "Ваш пароль має містити щонайменше 12 символів.",
    "Файл «звіт_вересень.xlsx» успішно завантажено",
    "Помилка 404: сторінку не знайдено",
    "Надіслати на v.plum@example.com о 14:35",
    "Картка 4539 1488 0343 6467, термін 09/28",
    "Оновлення Windows готове до встановлення",
    "Єдиний рахунок: UA21 3223 1300 0002 6007 2335 6600 1",
    "Підключено до мережі «Дім-5G» · 866 Мбіт/с",
    "Скасувати · Застосувати · Гаразд",
    "Ґудзик, їжа, ємність, п’ять, м’ята, сім’я",
    "Download completed — Завантаження завершено",
    "Вхідні (12) · Надіслані · Чернетки · Кошик",
    "Температура процесора: 67 °C, вентилятор 2150 об/хв",
]

PARAGRAPHS = [
    [
        "Шановний клієнте!",
        "Ваше замовлення № 48213 від 28.09.2026 відправлено.",
        "Трек-номер: 20450123456789. Очікуйте доставку",
        "протягом 1–3 робочих днів у відділення № 17.",
    ],
    [
        "Інструкція з налаштування:",
        "1. Відкрийте «Параметри» та виберіть «Мережа».",
        "2. Натисніть «Додати з’єднання» і введіть ключ.",
        "3. Перезапустіть застосунок, щоб зміни набули чинності.",
    ],
]

CODE = [
    'let назва = "Знімок екрана";',
    "fn main() { println!(\"Привіт, світе!\"); }",
    "api_key = sk-ant-api03-AbCdEf0123456789",
    "SELECT ім’я, прізвище FROM users WHERE id = 42;",
]

THEMES = [("light", (255, 255, 255), (20, 20, 20)), ("dark", (32, 32, 32), (230, 230, 230))]


def render(lines, font, bg, fg, pad=14, gap=1.35):
    asc, desc = font.getmetrics()
    h_line = int((asc + desc) * gap)
    w = max(int(font.getlength(line)) for line in lines) + 2 * pad
    img = Image.new("RGB", (w, pad * 2 + h_line * len(lines)), bg)
    d = ImageDraw.Draw(img)
    for i, line in enumerate(lines):
        d.text((pad, pad + i * h_line), line, font=font, fill=fg)
    return img


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    n = 0
    for fname, face in FACES:
        path = FONTS / fname
        if not path.exists():
            continue
        mono = face == "consolas"
        for size in (13, 15, 20):
            font = ImageFont.truetype(str(path), size)
            for theme, bg, fg in THEMES:
                groups = [[c] for c in CODE] if mono else [[l] for l in LINES] + PARAGRAPHS
                for gi, lines in enumerate(groups):
                    # Keep the set small: every line at every size only for Segoe UI (Windows UI).
                    if face != "segoe" and (gi % 3 != size % 3):
                        continue
                    name = f"{face}-{size}-{theme}-{gi:02d}"
                    render(lines, font, bg, fg).save(OUT / f"{name}.png", optimize=True)
                    (OUT / f"{name}.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
                    n += 1
    print(f"{n} pictures in {OUT}")


if __name__ == "__main__":
    main()
