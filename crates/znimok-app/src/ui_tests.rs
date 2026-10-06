//! UI tests on Slint's testing backend (ZK-33): no window, no GPU, so they run in `cargo test`
//! on both CI systems. They click through the accessibility tree the way a screen reader sees it;
//! everything that needs real windows, pixels or the OS stays in the self-test (`selftest.rs`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use i_slint_backend_testing::{AccessibleRole, ElementHandle};
use slint::ComponentHandle;
use slint::platform::PointerEventButton;

use crate::AppWindow;

/// One window per test thread (the testing backend is per thread).
fn window() -> AppWindow {
    thread_local! {
        static INIT: Cell<bool> = const { Cell::new(false) };
    }
    if !INIT.with(|i| i.replace(true)) {
        i_slint_backend_testing::init_no_event_loop();
    }
    let ui = AppWindow::new().expect("the window builds");
    // The labels below are the English source strings, whatever the machine's language (the
    // bundled translations are known once a component exists).
    slint::select_bundled_translation("en").expect("English is bundled");
    ui.window().set_size(slint::LogicalSize::new(1360.0, 860.0));
    ui.show().expect("shown on the testing backend");
    ui
}

/// The buttons a person can reach now: shown and not zero-sized.
fn buttons(ui: &AppWindow) -> Vec<ElementHandle> {
    i_slint_backend_testing::ElementQuery::from_root(ui)
        .match_descendants()
        .match_accessible_role(AccessibleRole::Button)
        .find_all()
        .into_iter()
        .filter(reachable)
        .collect()
}

/// Pages fade in: let the mock clock run past the animation.
fn settle() {
    i_slint_backend_testing::mock_elapsed_time(std::time::Duration::from_secs(1));
}

fn reachable(e: &ElementHandle) -> bool {
    let s = e.size();
    s.width > 0.0 && s.height > 0.0 && e.computed_opacity() > 0.0
}

fn by_label(ui: &AppWindow, label: &str) -> ElementHandle {
    ElementHandle::find_by_accessible_label(ui, label)
        .find(reachable)
        .unwrap_or_else(|| panic!("no reachable «{label}»"))
}

#[test]
fn a_tool_button_picks_its_tool() {
    let ui = window();
    ui.set_page(1);
    settle();
    let picked = Rc::new(RefCell::new(Vec::new()));
    {
        let picked = picked.clone();
        ui.on_tool_chosen(move |i| picked.borrow_mut().push(i));
    }
    by_label(&ui, "Ellipse (E)").mock_single_click(PointerEventButton::Left);
    by_label(&ui, "Text (T)").mock_single_click(PointerEventButton::Left);
    assert_eq!(*picked.borrow(), vec![2, 5]);
}

#[test]
fn updates_page_check_now_asks_the_app() {
    let ui = window();
    ui.set_page(2);
    ui.set_settings_page(9);
    settle();
    let asked = Rc::new(RefCell::new(Vec::new()));
    {
        let asked = asked.clone();
        ui.on_setting(move |k, v| asked.borrow_mut().push((k.to_string(), v)));
    }
    by_label(&ui, "Check now").mock_single_click(PointerEventButton::Left);
    assert_eq!(*asked.borrow(), vec![("upd-check".to_string(), 0)]);
    // While a check runs the button is disabled and a click does nothing.
    ui.set_upd_busy(true);
    by_label(&ui, "Check now").mock_single_click(PointerEventButton::Left);
    assert_eq!(asked.borrow().len(), 1);
}

/// Every button a person can reach has a name for screen readers — on the library, the editor
/// and every settings page.
#[test]
fn every_reachable_button_has_a_label() {
    let ui = window();
    let mut screens = vec![(0, 0, "library"), (1, 0, "editor")];
    screens.extend((0..=9).map(|p| (2, p, "settings")));
    let mut missing = Vec::new();
    let mut seen = 0;
    for (page, settings, name) in screens {
        ui.set_page(page);
        ui.set_settings_page(settings);
        settle();
        for b in buttons(&ui) {
            seen += 1;
            if b.accessible_label().is_none_or(|l| l.trim().is_empty()) {
                let p = b.absolute_position();
                missing.push(format!(
                    "{name} {settings}: {} at {:.0},{:.0}",
                    b.type_name().unwrap_or_default(),
                    p.x,
                    p.y
                ));
            }
        }
    }
    assert!(seen > 40, "only {seen} buttons found");
    assert!(
        missing.is_empty(),
        "buttons without a label:\n{}",
        missing.join("\n")
    );
}

/// ZK-277: the settings page scrolls to its real end. Its preferred height counts a wrapped text
/// as one line, so a long page (integrations switched on, several webhooks) was cut off at the
/// bottom — the last webhook's «Check» could not be reached.
#[test]
fn the_integrations_page_scrolls_to_its_end() {
    let ui = window();
    ui.window().set_size(slint::LogicalSize::new(1000.0, 700.0));
    ui.set_page(2);
    ui.set_settings_page(11);
    ui.set_int_tg_enabled(true);
    ui.set_int_jira_enabled(true);
    ui.set_int_slack_enabled(true);
    ui.set_int_rm_enabled(true);
    let hooks: Vec<crate::IntWebhook> = (0..3)
        .map(|i| crate::IntWebhook {
            id: format!("w{i}").into(),
            enabled: true,
            name: format!("Hook {i}").into(),
            ..Default::default()
        })
        .collect();
    ui.set_int_webhooks(std::rc::Rc::new(slint::VecModel::from(hooks)).into());
    let one = || -> slint::ModelRc<crate::IntAccount> {
        std::rc::Rc::new(slint::VecModel::from(vec![crate::IntAccount::default()])).into()
    };
    ui.set_int_tg_accounts(one());
    ui.set_int_jira_accounts(one());
    ui.set_int_slack_accounts(one());
    ui.set_int_rm_accounts(one());
    settle();
    ui.invoke_settings_scroll_end();
    settle();
    // The last webhook's «Check» (the page's last button) is on screen, whole.
    let bottom = ElementHandle::find_by_accessible_label(&ui, "Check")
        .filter(reachable)
        .map(|e| e.absolute_position().y + e.size().height)
        .fold(0.0f32, f32::max);
    assert!(bottom > 0.0, "no «Check» reachable at the end of the page");
    assert!(
        bottom <= 700.0,
        "the last «Check» ends at {bottom}, below the window"
    );
    // And it is the last webhook's: its «What to send» is above it.
    let last_what = ElementHandle::find_by_accessible_label(&ui, "What to send")
        .filter(reachable)
        .map(|e| e.absolute_position().y)
        .fold(0.0f32, f32::max);
    assert!(
        last_what > 0.0 && last_what < bottom,
        "what {last_what}, check {bottom}"
    );
}

/// ZK-279: the «Share» window — a service, then a place in it; «Send» only with a place.
#[test]
fn the_share_window_picks_a_service_and_a_place() {
    let ui = window();
    ui.set_page(1);
    let rows = |r: &[(&str, &str)]| -> slint::ModelRc<crate::IntTarget> {
        let v: Vec<crate::IntTarget> = r
            .iter()
            .map(|(k, n)| crate::IntTarget {
                key: (*k).into(),
                name: (*n).into(),
            })
            .collect();
        std::rc::Rc::new(slint::VecModel::from(v)).into()
    };
    ui.set_sh_targets(rows(&[
        ("slack", "Slack"),
        ("google", "Google Drive · a@b.c"),
    ]));
    ui.set_sh_target("slack".into());
    ui.set_sh_has_places(true);
    ui.set_sh_places(rows(&[("C1", "#general"), ("C2", "#design")]));
    ui.set_sh_whats(rows(&[
        ("image", "Picture"),
        ("document", "Znimok document"),
    ]));
    ui.set_sh_what("image".into());
    ui.set_sh_open(true);
    settle();
    let seen = Rc::new(RefCell::new(Vec::new()));
    {
        let seen = seen.clone();
        ui.on_sh_set(move |a, b| seen.borrow_mut().push(format!("{a}:{b}")));
    }
    // The place chosen stays in sight above the list, with ✕ to clear it (ZK-294).
    ui.set_sh_place("C1".into());
    ui.set_sh_place_name("Chosen: #general".into());
    settle();
    by_label(&ui, "Chosen: #general");
    by_label(&ui, "Clear the choice").mock_single_click(PointerEventButton::Left);
    by_label(&ui, "#design").mock_single_click(PointerEventButton::Left);
    by_label(&ui, "Google Drive · a@b.c").mock_single_click(PointerEventButton::Left);
    by_label(&ui, "Znimok document").mock_single_click(PointerEventButton::Left);
    // Without a place «Send» does nothing; with one it sends.
    by_label(&ui, "Send").mock_single_click(PointerEventButton::Left);
    ui.set_sh_can_send(true);
    by_label(&ui, "Send").mock_single_click(PointerEventButton::Left);
    assert_eq!(
        *seen.borrow(),
        [
            "place:",
            "place:C2",
            "target:google",
            "what:document",
            "send:"
        ]
    );
}

/// ZK-298: what is typed narrows the places and Enter takes it; a click on a row picks that row.
#[test]
fn typing_narrows_and_a_click_picks() {
    let ui = window();
    ui.set_page(1);
    let rows = |r: &[(&str, &str)]| -> slint::ModelRc<crate::IntTarget> {
        let v: Vec<crate::IntTarget> = r
            .iter()
            .map(|(k, n)| crate::IntTarget {
                key: (*k).into(),
                name: (*n).into(),
            })
            .collect();
        std::rc::Rc::new(slint::VecModel::from(v)).into()
    };
    ui.set_sh_targets(rows(&[("slack", "Slack")]));
    ui.set_sh_target("slack".into());
    ui.set_sh_has_places(true);
    ui.set_sh_places(rows(&[("ved", "Use «ved»"), ("D1", "@Vedmid")]));
    ui.set_sh_whats(rows(&[("image", "Picture")]));
    ui.set_sh_what("image".into());
    ui.set_sh_open(true);
    settle();
    let seen = Rc::new(RefCell::new(Vec::new()));
    {
        let seen = seen.clone();
        ui.on_sh_set(move |a, b| seen.borrow_mut().push(format!("{a}:{b}")));
    }
    let field = i_slint_backend_testing::ElementQuery::from_root(&ui)
        .match_descendants()
        .match_accessible_role(AccessibleRole::TextInput)
        .find_all()
        .into_iter()
        .find(reachable)
        .expect("the search field");
    field.mock_single_click(PointerEventButton::Left);
    for c in ["v", "e", "d", "\n"] {
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: c.into() });
        ui.window()
            .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: c.into() });
    }
    by_label(&ui, "@Vedmid").mock_single_click(PointerEventButton::Left);
    assert_eq!(
        *seen.borrow(),
        ["typed:v", "typed:ve", "typed:ved", "accept:ved", "place:D1"]
    );
}

/// ZK-280: a service with two accounts — the name of each, «Remove» on the added one only, and
/// the buttons say which account.
#[test]
fn a_service_with_two_accounts() {
    let ui = window();
    // Tall enough for the whole page: what is scrolled away is not in the tree.
    ui.window()
        .set_size(slint::LogicalSize::new(1360.0, 2400.0));
    ui.set_page(2);
    ui.set_settings_page(11);
    ui.set_int_slack_enabled(true);
    ui.set_int_slack_token_set(true);
    let accounts = vec![
        crate::IntAccount {
            name: "Plum".into(),
            token_set: true,
            ..Default::default()
        },
        crate::IntAccount {
            id: "c2".into(),
            name: "Client B".into(),
            ..Default::default()
        },
    ];
    ui.set_int_slack_accounts(std::rc::Rc::new(slint::VecModel::from(accounts)).into());
    // ZK-273: a build that can sign in to Slack shows the button on each account.
    ui.set_int_slack_sign_in(true);
    settle();
    let seen = Rc::new(RefCell::new(Vec::new()));
    {
        let seen = seen.clone();
        ui.on_int_action(move |a, b| seen.borrow_mut().push(format!("{a}:{b}")));
    }
    // A button is in the tree twice (the button and its label's text): the buttons only.
    let button = |e: &ElementHandle| e.accessible_role() == Some(AccessibleRole::Button);
    let removes: Vec<ElementHandle> = ElementHandle::find_by_accessible_label(&ui, "Remove")
        .filter(|e| reachable(e) && button(e))
        .collect();
    assert_eq!(removes.len(), 1, "«Remove» only on the added account");
    removes[0].mock_single_click(PointerEventButton::Left);
    by_label(&ui, "Add an account").mock_single_click(PointerEventButton::Left);
    let checks: Vec<ElementHandle> = ElementHandle::find_by_accessible_label(&ui, "Check")
        .filter(|e| reachable(e) && button(e))
        .collect();
    for c in &checks {
        c.mock_single_click(PointerEventButton::Left);
    }
    for b in ElementHandle::find_by_accessible_label(&ui, "Sign in to Slack")
        .filter(|e| reachable(e) && button(e))
    {
        b.mock_single_click(PointerEventButton::Left);
    }
    let seen = seen.borrow().clone();
    assert!(
        seen.contains(&"slack-sign-in:".to_string())
            && seen.contains(&"slack-sign-in:c2".to_string()),
        "{seen:?}"
    );
    assert!(
        seen.contains(&"remove-account:slack:c2".to_string()),
        "{seen:?}"
    );
    assert!(seen.contains(&"add-account:slack".to_string()), "{seen:?}");
    assert!(
        seen.contains(&"check:slack".to_string()) && seen.contains(&"check:slack:c2".to_string()),
        "{seen:?}"
    );
}
