//! A live check of the Telegram target through the OS HTTP stack (ZK-101):
//! `ZNIMOK_TG_TOKEN_FILE=<file with the bot token> cargo run -p znimok-share --example telegram_live [-- <chat id>]`.
//! Without a chat id it lists the chats that wrote to the bot; with one it sends a check message
//! and a small PNG. The token is never printed.

fn main() {
    let path = std::env::var("ZNIMOK_TG_TOKEN_FILE").expect("ZNIMOK_TG_TOKEN_FILE");
    let text = std::fs::read_to_string(path).expect("the token file");
    let token = text
        .split(|c: char| c.is_whitespace() || c == '`')
        .find(|w| {
            let mut p = w.splitn(2, ':');
            matches!((p.next(), p.next()), (Some(a), Some(b)) if a.len() >= 6 && a.bytes().all(|c| c.is_ascii_digit()) && b.len() >= 30)
        })
        .expect("a bot token in the file")
        .to_string();
    let t = znimok_models::http::system();
    let chat = std::env::args().nth(1);
    match chat {
        None => {
            println!(
                "{:?}",
                znimok_share::telegram::check(t.as_ref(), &token, "", "")
            );
            match znimok_share::telegram::find_chats(t.as_ref(), &token) {
                Ok(chats) => {
                    for (id, name) in chats {
                        println!("chat {id} — {name}");
                    }
                }
                Err(e) => println!("find_chats: {e}"),
            }
        }
        Some(chat) => {
            println!(
                "{:?}",
                znimok_share::telegram::check(
                    t.as_ref(),
                    &token,
                    &chat,
                    "Znimok: перевірка з'єднання ✓"
                )
            );
            // A 2×2 PNG.
            let png: &[u8] = &[
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, 0x49, 0x48, 0x44,
                0x52, 0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0, 0, 0, 0xFD, 0xD4, 0x9A, 0x73, 0, 0, 0, 0x16,
                0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x9F, 0x81, 0xE1,
                0x3F, 0x03, 0x03, 0x03, 0x00, 0x1D, 0xFE, 0x05, 0xFB, 0xE6, 0x7C, 0x17, 0x6B, 0, 0,
                0, 0, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
            ];
            let item = znimok_share::Item {
                file_name: "Знімок — перевірка.png".into(),
                mime: "image/png".into(),
                title: "Znimok · ZK-101".into(),
                text: "Тестовий файл з інтеграції Telegram".into(),
                kind: "screenshot".into(),
            };
            println!(
                "{:?}",
                znimok_share::telegram::send(t.as_ref(), &token, &chat, &item, png)
            );
        }
    }
}
