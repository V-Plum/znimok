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
