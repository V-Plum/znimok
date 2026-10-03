//! A live check of the Jira target through the OS HTTP stack (ZK-101):
//! `ZNIMOK_JIRA_CREDS=<file with «Site:», «Email:», «API Token:» lines> cargo run -p znimok-share --example jira_live -- <project> [issue]`.
//! Checks the connection, then sends a small PNG (a new issue in the project, or to the issue).
//! The token is never printed.

fn main() {
    let path = std::env::var("ZNIMOK_JIRA_CREDS").expect("ZNIMOK_JIRA_CREDS");
    let text = std::fs::read_to_string(path).expect("the credentials file");
    let field = |label: &str| {
        text.lines()
            .find_map(|l| {
                l.strip_prefix(label)
                    .map(|v| v.trim_start_matches(':').trim().to_string())
            })
            .unwrap_or_default()
    };
    let mut args = std::env::args().skip(1);
    let cfg = znimok_settings::JiraTarget {
        enabled: true,
        site: field("Site"),
        email: field("Email"),
        project: args.next().unwrap_or_default(),
        issue: args.next().unwrap_or_default(),
        issue_type: "Task".into(),
    };
    let token = field("API Token");
    let t = znimok_models::http::system();
    println!(
        "check: {:?}",
        znimok_share::jira::check(t.as_ref(), &cfg, &token)
    );
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, 0x49, 0x48, 0x44, 0x52, 0,
        0, 0, 2, 0, 0, 0, 2, 8, 2, 0, 0, 0, 0xFD, 0xD4, 0x9A, 0x73, 0, 0, 0, 0x16, 0x49, 0x44,
        0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x9F, 0x81, 0xE1, 0x3F, 0x03, 0x03,
        0x03, 0x00, 0x1D, 0xFE, 0x05, 0xFB, 0xE6, 0x7C, 0x17, 0x6B, 0, 0, 0, 0, 0x49, 0x45, 0x4E,
        0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let item = znimok_share::Item {
        file_name: "Знімок — перевірка.png".into(),
        mime: "image/png".into(),
        title: "Znimok · перевірка інтеграції Jira (ZK-101)".into(),
        text: "Тестовий файл із Znimok.\nДругий рядок опису.".into(),
        kind: "screenshot".into(),
    };
    println!(
        "send: {:?}",
        znimok_share::jira::send(t.as_ref(), &cfg, &token, &item, png)
    );
}
