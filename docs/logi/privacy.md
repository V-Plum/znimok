# Znimok plugin for Logi Options+ — privacy policy

> Draft (3 October 2026, ZK-258) — takes effect when the plugin is first published on the Logitech
> Marketplace, after the owner's review. Українською — нижче.

**In short:** the plugin collects no personal data, keeps no data of its own and makes no network
requests.

## What the plugin does with data

- It talks only to the Znimok application on the same computer, through a channel open to the current
  user alone (a named pipe on Windows, a Unix socket on macOS). Over it go the commands of your buttons
  ("take a screenshot", "start recording") and Znimok's state for the buttons (whether a recording runs
  and for how long, the active tool, the zoom) and its events for haptic feedback.
- It sends nothing to the author, to Logitech or to anyone else, and contains no analytics, telemetry
  or advertising.
- Screenshots, recordings and everything else you make stay with the Znimok application; how Znimok
  handles them is described in [Znimok's privacy policy](../privacy.en.md).
- The plugin writes a diagnostic log through Logi Plugin Service on your computer (action names and
  errors, no screen contents).

## Logitech

Installing the plugin from the Logitech Marketplace and running it in Logi Options+ is governed by
Logitech's own privacy policy.

## Contact

<https://github.com/V-Plum/znimok/issues>

---

# Плагін Znimok для Logi Options+ — політика конфіденційності

**Коротко:** плагін не збирає персональних даних, не зберігає власних даних і нічого не надсилає в
мережу.

- Він спілкується лише із застосунком Znimok на тому самому комп'ютері, каналом, відкритим тільки для
  поточного користувача: команди ваших кнопок і стан Znimok для кнопок та тактильного відгуку.
- Нічого не надсилає автору, Logitech чи будь-кому ще; аналітики, телеметрії й реклами немає.
- Знімки й записи лишаються в Znimok — див. [політику Znimok](../privacy.md).
- Діагностичний журнал (назви дій і помилки, без вмісту екрана) пише Logi Plugin Service на вашому
  комп'ютері.

Встановлення з Logitech Marketplace і робота в Logi Options+ — за політикою конфіденційності Logitech.
