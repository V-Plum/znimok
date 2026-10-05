//! Updates in the app (ZK-142), on top of `znimok-update` (ZK-122):
//!
//! - at start, the app confirms that this version came up (`mark_started`) — without it a
//!   running update rolls back to the previous version — and shows once what the last update did;
//! - with «Перевіряти щодня» on, a background check once a day; a newer release is reported once;
//! - «Встановити» (Windows): download and verify (signature, then checksum), hand the installer to
//!   `znimok.exe update install` and exit — it installs, starts the new version and rolls back if
//!   that does not come up.
//! - macOS (ZK-143): Sparkle 2 does the checking, the EdDSA-verified download, the installation
//!   and the relaunch; the page in the settings is Znimok's own (`mac` below, on
//!   `znimok_mac::sparkle`). Without the framework in the bundle (a dev build) or a feed, the
//!   page says updates are not set up.
//!
//! Nothing touches the network unless the person turned the daily check on or pressed a button.

use std::path::PathBuf;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `…\Znimok\Updates` — the same folder the command line uses.
pub fn updates_dir() -> PathBuf {
    znimok_agents::data_dir().join("Updates")
}

/// The app is up: confirm the start of this version to a running update.
pub fn confirm_start() {
    let _ = znimok_update::apply::mark_started(&updates_dir(), VERSION);
}

/// What the last update did, once (ZK-211: only the note about this very version, or a fresh
/// rollback / failure; the note is moved aside after it is read).
pub fn take_outcome() -> Option<znimok_update::apply::Outcome> {
    let now = chrono::Utc::now().timestamp_millis();
    znimok_update::apply::take_outcome(&updates_dir(), VERSION, now, 24 * 3600 * 1000)
}

/// Shows the updater's note in the window when there is one; whether there was.
pub fn show_outcome() -> bool {
    let Some(outcome) = take_outcome() else {
        return false;
    };
    crate::with_ctx(|a, ui| {
        use znimok_update::apply::Outcome;
        let f =
            |k: &str, pairs: &[(&'static str, String)]| a.tr.tr_args(k, &crate::app::fargs(pairs));
        let text = match outcome {
            Outcome::Installed { version } => f("upd-outcome-installed", &[("version", version)]),
            Outcome::RolledBack { version, reason } => f(
                "upd-outcome-rolled-back",
                &[("version", version), ("reason", reason)],
            ),
            Outcome::Failed { reason } => f("upd-outcome-failed", &[("reason", reason)]),
        };
        let (title, close) = (a.tr.tr("upd-outcome-title"), a.tr.tr("common-close"));
        crate::dialog::ask(ui, title, text, vec![close], 0, Some(0), |_, _| {});
    });
    true
}

/// The result of a check.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    UpToDate,
    /// Windows: a release with an installer to download.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Available(znimok_update::Available),
    /// No release key in this build yet (ZK-111).
    NotConfigured,
    Failed(String),
    /// macOS: what Sparkle found (`size` in bytes; `notes` the release page).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Sparkle {
        version: String,
        notes: Option<String>,
        size: u64,
    },
}

/// Where a Sparkle update stands (macOS), for the page.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum Phase {
    Idle,
    Downloading { received: u64, total: u64 },
    Extracting(f64),
    Installing,
}

/// The macOS update flow as a state machine (ZK-297), apart from Sparkle so it is tested on every
/// system. One per process, shared by every window: a check begun in one window and answered
/// while another polls must not leave the first one «checking» for ever.
///
/// What it guards against — each was a way to hang on «Checking…» until a restart:
/// - a background check that finds nothing or fails says nothing to Sparkle's driver; only the
///   end of the cycle comes ([`Ev::CycleEnd`]);
/// - a check asked for while a found update waits for its answer only brings that update «into
///   focus» ([`Ev::InFocus`]) — and a background one is refused without a word;
/// - whatever else: the flow is busy with a check only while Sparkle itself has a session.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub mod flow {
    use std::time::{Duration, Instant};

    use super::{Found, Phase};

    /// `SUNoUpdateError`, and the person's own choices (`SUInstallationCanceledError`,
    /// `SUInstallationAuthorizeLaterError`).
    pub const NO_UPDATE: i64 = 1001;
    pub const CANCELED: i64 = 4007;
    pub const AUTHORIZE_LATER: i64 = 4008;

    /// A failed background check is tried again not sooner than this.
    pub const RETRY: Duration = Duration::from_secs(30 * 60);
    /// Background checks are never closer to each other than this, whatever went wrong.
    pub const BACKGROUND_GAP: Duration = Duration::from_secs(5 * 60);
    /// A check with no end after this long is cancelled.
    pub const WATCHDOG: Duration = Duration::from_secs(120);
    /// Sparkle has no session and said nothing for this long: the check is over.
    pub const SETTLE: Duration = Duration::from_secs(3);

    /// What Sparkle said, as the flow needs it.
    #[derive(Clone, Debug, PartialEq)]
    pub enum Ev {
        Checking,
        Found {
            version: String,
            notes: Option<String>,
            size: u64,
        },
        NotFound,
        Error(String),
        DownloadStarted,
        DownloadTotal(u64),
        DownloadProgress(u64),
        Extracting(f64),
        ReadyToInstall,
        Installing {
            app_terminated: bool,
        },
        Installed,
        Dismissed,
        InFocus,
        CycleEnd {
            background: bool,
            error: Option<(i64, String)>,
        },
    }

    /// Sparkle as the flow sees it.
    pub trait Updater {
        fn can_check(&self) -> bool;
        fn session_in_progress(&self) -> bool;
        /// A found update (or a downloaded one) waits for the person's answer.
        fn awaits_reply(&self) -> bool;
        fn check(&self, user_initiated: bool);
        /// Answers the pending question with «install»; whether there was one.
        fn install(&self) -> bool;
        /// Answers the pending question with «later»; whether there was one.
        fn dismiss(&self) -> bool;
        /// Cancels a running check or download, when Sparkle gave a way to.
        fn cancel(&self);
    }

    /// What the app does after a step.
    #[derive(Clone, Debug)]
    pub enum Effect {
        /// A check has an answer: remember when, tell the person once. `Failed("")` = no answer.
        Checked(Box<Found>),
        /// The update is about to be installed: save every document.
        SaveAll,
        /// Sparkle's installer waits for the app to quit.
        Quit,
    }

    #[derive(Debug)]
    pub struct Flow {
        pub busy: bool,
        pub phase: Phase,
        pub found: Option<Found>,
        /// The person pressed «Check now» and waits for its answer.
        pub user_waits: bool,
        /// A found update is being dismissed to check anew.
        recheck: bool,
        /// The running check already has its answer (the cycle's end adds nothing).
        settled: bool,
        /// When the running check began, and when Sparkle was last seen busy with it.
        since: Option<Instant>,
        retry_at: Option<Instant>,
        last_background: Option<Instant>,
    }

    impl Default for Flow {
        fn default() -> Self {
            Self {
                busy: false,
                phase: Phase::Idle,
                found: None,
                user_waits: false,
                recheck: false,
                settled: false,
                since: None,
                retry_at: None,
                last_background: None,
            }
        }
    }

    impl Flow {
        fn settle(&mut self, found: Found, fx: &mut Vec<Effect>) {
            self.busy = false;
            self.user_waits = false;
            self.since = None;
            self.settled = true;
            self.phase = Phase::Idle;
            self.found = Some(found.clone());
            fx.push(Effect::Checked(Box::new(found)));
        }

        fn begin(&mut self, u: &dyn Updater, user: bool, now: Instant, fx: &mut Vec<Effect>) {
            self.settled = false;
            if !user {
                self.last_background = Some(now);
            }
            u.check(user);
            if u.session_in_progress() {
                self.busy = true;
                self.user_waits = user;
                self.since = Some(now);
            } else if user {
                // Refused without a word.
                self.settle(Found::Failed(String::new()), fx);
            }
        }

        /// The daily check may start now.
        pub fn may_check_in_background(&self, now: Instant) -> bool {
            !self.busy
                && self.retry_at.is_none_or(|t| now >= t)
                && self
                    .last_background
                    .is_none_or(|t| now.duration_since(t) >= BACKGROUND_GAP)
        }

        /// «Check now» (`user`), or the daily check.
        pub fn press(&mut self, u: &dyn Updater, user: bool, now: Instant) -> Vec<Effect> {
            let mut fx = Vec::new();
            if self.busy {
                // A check runs: its end answers the person too. A download goes on.
                if user && self.phase == Phase::Idle {
                    self.user_waits = true;
                }
                return fx;
            }
            if u.awaits_reply() {
                // A found update waits for its answer: Sparkle would only bring it «into focus»
                // (and refuse a background check). The person asks anew — it may be older than
                // what is out now: put it away, then check.
                if user {
                    self.recheck = true;
                    self.busy = true;
                    self.user_waits = true;
                    self.settled = false;
                    self.since = Some(now);
                    if !u.dismiss() {
                        self.recheck = false;
                        self.begin(u, true, now, &mut fx);
                    }
                }
                return fx;
            }
            if u.session_in_progress() {
                // A cycle the flow did not begin: wait for its end.
                if user {
                    self.busy = true;
                    self.user_waits = true;
                    self.since = Some(now);
                }
                return fx;
            }
            if !u.can_check() {
                if user {
                    self.settle(Found::Failed(String::new()), &mut fx);
                }
                return fx;
            }
            self.begin(u, user, now, &mut fx);
            fx
        }

        /// «Install»: the found update's answer.
        pub fn install(&mut self, u: &dyn Updater) -> bool {
            if self.busy || !matches!(self.found, Some(Found::Sparkle { .. })) || !u.install() {
                return false;
            }
            self.busy = true;
            self.user_waits = false;
            self.phase = Phase::Downloading {
                received: 0,
                total: 0,
            };
            true
        }

        /// What Sparkle said since the last step (maybe nothing), and Sparkle's own state.
        pub fn step(&mut self, u: &dyn Updater, events: Vec<Ev>, now: Instant) -> Vec<Effect> {
            let mut fx = Vec::new();
            for e in events {
                match e {
                    Ev::Checking => {
                        self.busy = true;
                        self.since.get_or_insert(now);
                    }
                    Ev::Found {
                        version,
                        notes,
                        size,
                    } => {
                        self.recheck = false;
                        self.settle(
                            Found::Sparkle {
                                version,
                                notes,
                                size,
                            },
                            &mut fx,
                        );
                    }
                    Ev::NotFound => self.settle(Found::UpToDate, &mut fx),
                    Ev::Error(e) => self.settle(Found::Failed(e), &mut fx),
                    Ev::DownloadStarted => {
                        self.busy = true;
                        self.phase = Phase::Downloading {
                            received: 0,
                            total: 0,
                        };
                    }
                    Ev::DownloadTotal(t) => {
                        if let Phase::Downloading { total, .. } = &mut self.phase {
                            *total = t;
                        }
                    }
                    Ev::DownloadProgress(n) => {
                        if let Phase::Downloading { received, .. } = &mut self.phase {
                            *received += n;
                        }
                    }
                    Ev::Extracting(p) => {
                        self.busy = true;
                        self.phase = Phase::Extracting(p);
                    }
                    Ev::ReadyToInstall => {
                        // «Install» means the whole way, as on Windows: save, then relaunch.
                        fx.push(Effect::SaveAll);
                        self.phase = Phase::Installing;
                        u.install();
                    }
                    Ev::Installing { app_terminated } => {
                        self.phase = Phase::Installing;
                        if !app_terminated {
                            fx.push(Effect::Quit);
                        }
                    }
                    Ev::Installed => {
                        self.phase = Phase::Idle;
                        self.busy = false;
                    }
                    Ev::Dismissed => {
                        self.phase = Phase::Idle;
                        if !self.recheck {
                            self.busy = false;
                        }
                    }
                    // The found update is on the page already.
                    Ev::InFocus => {
                        if !self.recheck {
                            self.busy = false;
                            self.user_waits = false;
                        }
                    }
                    Ev::CycleEnd { background, error } => {
                        if self.recheck {
                            // The old found update is put away: now the check the person asked for.
                            self.recheck = false;
                            self.found = None;
                            self.busy = false;
                            self.begin(u, true, now, &mut fx);
                            continue;
                        }
                        match error {
                            // After an update that was shown and answered: nothing to add.
                            None => {}
                            Some((NO_UPDATE, _)) => {
                                if !self.settled {
                                    self.settle(Found::UpToDate, &mut fx);
                                }
                            }
                            Some((CANCELED | AUTHORIZE_LATER, _)) => {}
                            Some((_, text)) => {
                                if self.settled {
                                } else if self.user_waits || !background {
                                    self.settle(Found::Failed(text), &mut fx);
                                } else {
                                    // The daily check could not reach the feed (no network after
                                    // a wake): quietly, and again later.
                                    self.retry_at = Some(now + RETRY);
                                }
                            }
                        }
                        // A cycle is over: nothing runs, whatever was or was not said before.
                        if self.phase != Phase::Installing {
                            self.phase = Phase::Idle;
                            self.busy = false;
                        }
                        self.user_waits = false;
                        self.since = None;
                    }
                }
            }
            // Sparkle's own state is the truth: busy with a check only while it has a session.
            if self.busy && self.phase == Phase::Idle && !self.recheck {
                if u.session_in_progress() || u.awaits_reply() {
                    if self.since.is_some_and(|t| now.duration_since(t) > WATCHDOG) {
                        u.cancel();
                        self.since = Some(now);
                    }
                } else if self.since.is_none_or(|t| now.duration_since(t) > SETTLE) {
                    if self.user_waits && !self.settled {
                        self.settle(Found::Failed(String::new()), &mut fx);
                    } else {
                        self.busy = false;
                        self.user_waits = false;
                        self.since = None;
                        self.retry_at.get_or_insert(now + RETRY);
                    }
                }
            }
            fx
        }
    }

    #[cfg(test)]
    mod tests {
        use std::cell::{Cell, RefCell};

        use super::*;

        /// Sparkle as the live test saw it behave (crates/znimok-mac/tests/sparkle_live.rs).
        #[derive(Default)]
        struct Fake {
            session: Cell<bool>,
            pending: Cell<bool>,
            calls: RefCell<Vec<&'static str>>,
            refuse: Cell<bool>,
        }

        impl Updater for Fake {
            fn can_check(&self) -> bool {
                !self.session.get() || self.pending.get()
            }
            fn session_in_progress(&self) -> bool {
                self.session.get()
            }
            fn awaits_reply(&self) -> bool {
                self.pending.get()
            }
            fn check(&self, user: bool) {
                self.calls
                    .borrow_mut()
                    .push(if user { "check" } else { "check-bg" });
                if !self.refuse.get() && !self.session.get() {
                    self.session.set(true);
                }
            }
            fn install(&self) -> bool {
                self.calls.borrow_mut().push("install");
                self.pending.replace(false)
            }
            fn dismiss(&self) -> bool {
                self.calls.borrow_mut().push("dismiss");
                self.pending.replace(false)
            }
            fn cancel(&self) {
                self.calls.borrow_mut().push("cancel");
            }
        }

        fn end(background: bool, error: Option<(i64, &str)>) -> Ev {
            Ev::CycleEnd {
                background,
                error: error.map(|(c, t)| (c, t.to_string())),
            }
        }

        fn checked(fx: &[Effect]) -> Vec<String> {
            fx.iter()
                .filter_map(|e| match e {
                    Effect::Checked(f) => Some(match &**f {
                        Found::UpToDate => "up-to-date".to_string(),
                        Found::Failed(e) => format!("failed:{e}"),
                        Found::Sparkle { version, .. } => format!("found:{version}"),
                        other => format!("{other:?}"),
                    }),
                    _ => None,
                })
                .collect()
        }

        /// The owner's case: the daily check after a wake finds nothing (or has no network) and
        /// Sparkle's driver hears nothing — «Check now» stayed off until a restart.
        #[test]
        fn a_quiet_background_check_ends() {
            let (u, mut f, t0) = (Fake::default(), Flow::default(), Instant::now());
            assert!(f.may_check_in_background(t0));
            assert!(f.press(&u, false, t0).is_empty());
            assert!(f.busy && !f.user_waits);
            // Nothing newer: the cycle's end is the only word.
            u.session.set(false);
            let fx = f.step(&u, vec![end(true, Some((NO_UPDATE, "up to date")))], t0);
            assert_eq!(checked(&fx), ["up-to-date"]);
            assert!(!f.busy);
            // No network: quiet, free at once, and not tried again every tick.
            let t1 = t0 + BACKGROUND_GAP;
            f.press(&u, false, t1);
            u.session.set(false);
            let fx = f.step(&u, vec![end(true, Some((2001, "offline")))], t1);
            assert!(checked(&fx).is_empty() && !f.busy);
            assert!(!f.may_check_in_background(t1 + Duration::from_secs(60)));
            assert!(f.may_check_in_background(t1 + RETRY));
            // «Check now» works straight away.
            let fx = f.press(&u, true, t1);
            assert!(fx.is_empty() && f.busy && f.user_waits);
            u.session.set(false);
            let fx = f.step(
                &u,
                vec![
                    Ev::Checking,
                    Ev::NotFound,
                    Ev::Dismissed,
                    end(false, Some((NO_UPDATE, ""))),
                ],
                t1,
            );
            assert_eq!(
                checked(&fx),
                ["up-to-date"],
                "told once, not again by the cycle's end"
            );
            assert!(!f.busy);
        }

        /// The person presses «Check now» while the daily check runs: its end answers them.
        #[test]
        fn a_press_during_a_background_check_gets_its_answer() {
            let (u, mut f, t0) = (Fake::default(), Flow::default(), Instant::now());
            f.press(&u, false, t0);
            f.press(&u, true, t0);
            assert!(f.user_waits);
            assert_eq!(
                *u.calls.borrow(),
                ["check-bg"],
                "no second check over the first"
            );
            u.session.set(false);
            let fx = f.step(&u, vec![end(true, Some((2001, "offline")))], t0);
            assert_eq!(checked(&fx), ["failed:offline"]);
            assert!(!f.busy);
        }

        /// A found update waits for its answer: the daily tick leaves it alone, «Check now»
        /// puts it away and checks anew (it may be older than what is out).
        #[test]
        fn a_pending_update_and_a_new_check() {
            let (u, mut f, t0) = (Fake::default(), Flow::default(), Instant::now());
            f.press(&u, false, t0);
            u.pending.set(true);
            let fx = f.step(
                &u,
                vec![Ev::Found {
                    version: "0.0.20".into(),
                    notes: None,
                    size: 1,
                }],
                t0,
            );
            assert_eq!(checked(&fx), ["found:0.0.20"]);
            assert!(!f.busy);
            // A day later the daily check: nothing is asked of Sparkle, nothing hangs.
            let t1 = t0 + Duration::from_secs(86_400);
            assert!(f.press(&u, false, t1).is_empty());
            assert!(!f.busy);
            assert_eq!(*u.calls.borrow(), ["check-bg"]);
            // «Check now»: dismissed first, the check after the cycle's end.
            f.press(&u, true, t1);
            assert!(f.busy && f.user_waits);
            assert_eq!(u.calls.borrow().last(), Some(&"dismiss"));
            u.session.set(false);
            let fx = f.step(&u, vec![Ev::Dismissed, end(true, None)], t1);
            assert!(fx.is_empty());
            assert_eq!(u.calls.borrow().last(), Some(&"check"));
            assert!(f.busy && f.found.is_none());
            u.pending.set(true);
            let fx = f.step(
                &u,
                vec![
                    Ev::Checking,
                    Ev::Found {
                        version: "0.0.21".into(),
                        notes: None,
                        size: 1,
                    },
                ],
                t1,
            );
            assert_eq!(checked(&fx), ["found:0.0.21"]);
            // «Install» → download → ready → the app saves and quits for the installer.
            assert!(f.install(&u));
            assert!(f.busy);
            let fx = f.step(
                &u,
                vec![
                    Ev::DownloadStarted,
                    Ev::DownloadTotal(10),
                    Ev::DownloadProgress(10),
                    Ev::Extracting(1.0),
                    Ev::ReadyToInstall,
                    Ev::Installing {
                        app_terminated: false,
                    },
                ],
                t1,
            );
            assert!(matches!(fx[0], Effect::SaveAll) && matches!(fx[1], Effect::Quit));
            assert_eq!(f.phase, Phase::Installing);
        }

        /// Whatever Sparkle did not say: busy with a check only while it has a session; a check
        /// that never ends is cancelled; a refused one says so.
        #[test]
        fn sparkle_s_own_state_is_the_truth() {
            let (u, mut f, t0) = (Fake::default(), Flow::default(), Instant::now());
            f.press(&u, true, t0);
            // The session is gone and no event came.
            u.session.set(false);
            assert!(f.step(&u, vec![], t0 + Duration::from_secs(1)).is_empty());
            assert!(f.busy, "a moment for the events to come");
            let fx = f.step(&u, vec![], t0 + SETTLE + Duration::from_secs(1));
            assert_eq!(checked(&fx), ["failed:"]);
            assert!(!f.busy);
            // A check that hangs: cancelled after the watchdog.
            let t1 = t0 + Duration::from_secs(600);
            f.press(&u, true, t1);
            f.step(&u, vec![], t1 + WATCHDOG + Duration::from_secs(1));
            assert_eq!(u.calls.borrow().last(), Some(&"cancel"));
            u.session.set(false);
            let fx = f.step(
                &u,
                vec![end(false, Some((CANCELED, "")))],
                t1 + WATCHDOG + Duration::from_secs(2),
            );
            assert!(!f.busy && checked(&fx).is_empty());
            // Sparkle refuses the check without a word.
            u.refuse.set(true);
            let fx = f.press(&u, true, t1 + Duration::from_secs(900));
            assert_eq!(checked(&fx), ["failed:"]);
            assert!(!f.busy);
            // «Into focus» alone frees the page as well.
            let mut f = Flow {
                busy: true,
                user_waits: true,
                ..Flow::default()
            };
            u.session.set(true);
            u.pending.set(true);
            f.step(&u, vec![Ev::InFocus], t1);
            assert!(!f.busy);
        }
    }
}

#[cfg(target_os = "macos")]
pub mod mac {
    //! The one Sparkle updater of the process, on the main thread, and the one [`flow::Flow`]
    //! every window shows (ZK-297).
    use std::cell::RefCell;
    use std::time::Instant;

    use znimok_mac::sparkle::{Choice, Event, Sparkle, framework_in_bundle};

    use super::flow::{Effect, Ev, Flow, Updater};
    use super::{Found, Phase};

    thread_local! {
        static SPARKLE: RefCell<Option<Sparkle>> = const { RefCell::new(None) };
        static FLOW: RefCell<Flow> = RefCell::new(Flow::default());
    }

    impl Updater for Sparkle {
        fn can_check(&self) -> bool {
            Sparkle::can_check(self)
        }
        fn session_in_progress(&self) -> bool {
            Sparkle::session_in_progress(self)
        }
        fn awaits_reply(&self) -> bool {
            Sparkle::awaits_reply(self)
        }
        fn check(&self, user_initiated: bool) {
            Sparkle::check(self, user_initiated);
        }
        fn install(&self) -> bool {
            self.reply(Choice::Install)
        }
        fn dismiss(&self) -> bool {
            self.reply(Choice::Dismiss)
        }
        fn cancel(&self) {
            Sparkle::cancel(self);
        }
    }

    /// Start Sparkle when the bundle carries it and a feed is set (`SUFeedURL`, or
    /// `ZNIMOK_SPARKLE_FEED` in the environment for tests). `false` = updates not set up.
    pub fn init() -> bool {
        let Some(fw) = framework_in_bundle() else {
            return false;
        };
        let feed = std::env::var("ZNIMOK_SPARKLE_FEED").ok();
        match Sparkle::start(&fw, None, feed.as_deref()) {
            Ok(s) => {
                SPARKLE.with(|c| *c.borrow_mut() = Some(s));
                true
            }
            Err(e) => {
                eprintln!("Sparkle: {e}");
                false
            }
        }
    }

    pub fn available() -> bool {
        SPARKLE.with(|c| c.borrow().is_some())
    }

    fn with<R>(f: impl FnOnce(&Sparkle, &mut Flow) -> R) -> Option<R> {
        SPARKLE.with(|c| {
            let s = c.borrow();
            let s = s.as_ref()?;
            Some(FLOW.with(|fl| f(s, &mut fl.borrow_mut())))
        })
    }

    fn ev(e: Event) -> Option<Ev> {
        Some(match e {
            Event::Checking => Ev::Checking,
            Event::Found {
                version,
                notes_url,
                size,
                ..
            } => Ev::Found {
                version,
                notes: notes_url,
                size,
            },
            Event::NotesFailed(_) => return None,
            Event::NotFound(_) => Ev::NotFound,
            Event::Error(e) => Ev::Error(e),
            Event::DownloadStarted => Ev::DownloadStarted,
            Event::DownloadTotal(t) => Ev::DownloadTotal(t),
            Event::DownloadProgress(n) => Ev::DownloadProgress(n),
            Event::Extracting(p) => Ev::Extracting(p),
            Event::ReadyToInstall => Ev::ReadyToInstall,
            Event::Installing { app_terminated } => Ev::Installing { app_terminated },
            Event::InstalledAndRelaunched(_) => Ev::Installed,
            Event::Dismissed => Ev::Dismissed,
            Event::InFocus => Ev::InFocus,
            Event::CycleFinished { background, error } => Ev::CycleEnd { background, error },
        })
    }

    /// What Sparkle said since the last tick, and its own state, into the flow.
    pub fn step() -> Vec<Effect> {
        with(|s, f| {
            let events: Vec<Ev> = s.poll().into_iter().filter_map(ev).collect();
            if !events.is_empty() {
                tracing::info!(?events, "updates: Sparkle");
            }
            let was = f.busy;
            let fx = f.step(s, events, Instant::now());
            if was && !f.busy {
                tracing::info!(found = ?f.found, "updates: the check is over");
            }
            fx
        })
        .unwrap_or_default()
    }

    /// «Check now» (`user_initiated`), or the daily check.
    pub fn press(user_initiated: bool) -> Vec<Effect> {
        with(|s, f| {
            let fx = f.press(s, user_initiated, Instant::now());
            tracing::info!(user_initiated, busy = f.busy, "updates: a check asked for");
            fx
        })
        .unwrap_or_default()
    }

    pub fn install() -> bool {
        with(|s, f| f.install(s)).unwrap_or(false)
    }

    pub fn may_check_in_background() -> bool {
        FLOW.with(|f| f.borrow().may_check_in_background(Instant::now()))
    }

    /// The flow as the page shows it: busy, the person waits for a check, the phase, the answer.
    pub fn state() -> (bool, bool, Phase, Option<Found>) {
        FLOW.with(|f| {
            let f = f.borrow();
            (f.busy, f.user_waits, f.phase.clone(), f.found.clone())
        })
    }
}

/// Checks on a worker thread; `then` runs on the UI thread (Windows; macOS asks Sparkle).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn check(then: impl FnOnce(Found) + Send + 'static) {
    std::thread::spawn(move || {
        // Without a release key the answer needs no HTTP client (making one may take long:
        // proxy discovery on Windows).
        let found = match znimok_update::Platform::current() {
            _ if !znimok_update::configured() => Found::NotConfigured,
            None => Found::Failed("no installer for this system".into()),
            Some(platform) => {
                let http = znimok_models::http::system();
                match znimok_update::check(http.as_ref(), VERSION, platform) {
                    Ok(Some(a)) => Found::Available(a),
                    Ok(None) => Found::UpToDate,
                    Err(znimok_update::UpdateError::NotConfigured) => Found::NotConfigured,
                    Err(e) => Found::Failed(e.to_string()),
                }
            }
        };
        let _ = slint::invoke_from_event_loop(move || then(found));
    });
}

/// Windows: downloads and verifies the installer, starts `znimok.exe update install` (it waits for
/// this process to exit) and asks the app to quit. `failed` gets the reason otherwise.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn install(a: znimok_update::Available, failed: impl FnOnce(String) + Send + 'static) {
    std::thread::spawn(move || {
        let http = znimok_models::http::system();
        let r = znimok_update::download(http.as_ref(), &a, VERSION, &updates_dir())
            .map_err(|e| e.to_string())
            .and_then(|msi| run_installer(&msi, &a.version));
        let _ = slint::invoke_from_event_loop(move || match r {
            Ok(()) => {
                let _ = slint::quit_event_loop();
            }
            Err(e) => failed(e),
        });
    });
}

fn run_installer(msi: &std::path::Path, version: &str) -> Result<(), String> {
    let app = std::env::current_exe().map_err(|e| e.to_string())?;
    let cli = app.with_file_name(if cfg!(windows) {
        "znimok.exe"
    } else {
        "znimok"
    });
    if !cli.exists() {
        return Err(format!("{} is missing", cli.display()));
    }
    std::process::Command::new(&cli)
        .args(["update", "install", "--msi"])
        .arg(msi)
        .args(["--version", version, "--wait-pid"])
        .arg(std::process::id().to_string())
        .arg("--app")
        .arg(&app)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", cli.display()))
}

/// The release page in the browser (macOS until Sparkle, ZK-143). Only GitHub pages of the repo.
pub fn open_page(url: &str) {
    if !url.starts_with("https://github.com/V-Plum/znimok/") {
        return;
    }
    #[cfg(windows)]
    let _ = std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
}
