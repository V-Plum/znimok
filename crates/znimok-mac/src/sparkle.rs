//! Sparkle 2 with Znimok's own window (ZK-143, decision 33): the framework does the checking,
//! downloading, EdDSA verification, installation and relaunch; the app draws every step itself.
//!
//! - `Sparkle.framework` is loaded at run time from the bundle's `Frameworks` folder (no link
//!   step, no rpath): without it — a dev build, a bare binary — there is simply no updater.
//! - [`Driver`] implements `SPUUserDriver`: every call becomes an [`Event`] for the app; the
//!   two questions Sparkle asks (install this update? relaunch now?) keep their reply blocks
//!   until the app answers with [`Sparkle::reply`].
//! - [`Delegate`] answers `feedURLStringForUpdater:` when a feed is given in code (dev builds,
//!   tests); releases carry `SUFeedURL` in Info.plist.
//! - Automatic checks are off in Sparkle; the app asks ([`Sparkle::check`]) on its own daily
//!   timer, as on Windows (security review: nothing goes to the network unless the person
//!   turned the daily check on or pressed the button).
//!
//! Sparkle calls the driver on the main thread; the app polls [`Sparkle::poll`] from its timer,
//! also on the main thread.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use block2::{Block, RcBlock};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{MainThreadMarker, NSBundle, NSError, NSObjectProtocol, NSString, NSURL};

/// What the person may answer (`SPUUserUpdateChoice`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Skip = 0,
    Install = 1,
    Dismiss = 2,
}

/// Where an update stands when it is shown (`SPUUserUpdateStage`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    NotDownloaded,
    Downloaded,
    Installing,
}

/// What Sparkle told the driver, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A check the person started is running (cancel with [`Sparkle::cancel`]).
    Checking,
    /// A newer version. The reply is pending until [`Sparkle::reply`].
    Found {
        version: String,
        notes_url: Option<String>,
        size: u64,
        critical: bool,
        stage: Stage,
        user_initiated: bool,
    },
    /// Release notes could not be shown (informational).
    NotesFailed(String),
    /// No newer version (after a check the person started); Sparkle's message.
    NotFound(String),
    Error(String),
    DownloadStarted,
    DownloadTotal(u64),
    /// Bytes received since the last event.
    DownloadProgress(u64),
    Extracting(f64),
    /// Downloaded and verified; the reply (relaunch now?) is pending until [`Sparkle::reply`].
    ReadyToInstall,
    /// Installing; `app_terminated` false = Sparkle waits for the app to quit
    /// ([`Sparkle::retry_terminate`] asks it again).
    Installing {
        app_terminated: bool,
    },
    InstalledAndRelaunched(bool),
    /// Everything torn down (the person dismissed, or the session ended).
    Dismissed,
}

struct DriverIvars {
    tx: Sender<Event>,
    found: RefCell<Option<RcBlock<dyn Fn(isize)>>>,
    ready: RefCell<Option<RcBlock<dyn Fn(isize)>>>,
    cancel: RefCell<Option<RcBlock<dyn Fn()>>>,
    retry: RefCell<Option<RcBlock<dyn Fn()>>>,
}

fn text(s: Option<Retained<NSString>>) -> String {
    s.map(|s| s.to_string()).unwrap_or_default()
}

fn error_text(e: &NSError) -> String {
    let d = e.localizedDescription().to_string();
    if d.is_empty() {
        format!("{} ({})", e.domain(), e.code())
    } else {
        d
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the ivars are initialised in `new`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZnimokSparkleDriver"]
    #[ivars = DriverIvars]
    struct Driver;

    unsafe impl NSObjectProtocol for Driver {}

    // SPUUserDriver. Every required method of the protocol is here: a missing one would be an
    // unrecognised selector inside Sparkle.
    impl Driver {
        #[unsafe(method(showUpdatePermissionRequest:reply:))]
        fn show_permission(&self, _request: &AnyObject, reply: &Block<dyn Fn(*mut AnyObject)>) {
            // No automatic checks, no system profile: the app decides when to check.
            if let Some(cls) = AnyClass::get(c"SUUpdatePermissionResponse") {
                // SAFETY: documented initialiser of a Sparkle class.
                let response: Retained<AnyObject> = unsafe {
                    let a: Allocated<AnyObject> = msg_send![cls, alloc];
                    msg_send![a, initWithAutomaticUpdateChecks: false, sendSystemProfile: false]
                };
                reply.call((Retained::as_ptr(&response) as *mut AnyObject,));
            }
        }

        #[unsafe(method(showUserInitiatedUpdateCheckWithCancellation:))]
        fn show_check(&self, cancellation: &Block<dyn Fn()>) {
            self.ivars().cancel.replace(Some(cancellation.copy()));
            let _ = self.ivars().tx.send(Event::Checking);
        }

        #[unsafe(method(showUpdateFoundWithAppcastItem:state:reply:))]
        fn show_found(&self, item: &AnyObject, state: &AnyObject, reply: &Block<dyn Fn(isize)>) {
            // SAFETY: documented readonly properties of SUAppcastItem and SPUUserUpdateState.
            let (version, notes_url, size, critical, stage, user_initiated) = unsafe {
                let v: Option<Retained<NSString>> = msg_send![item, displayVersionString];
                let notes: Option<Retained<NSURL>> = msg_send![item, releaseNotesURL];
                let info: Option<Retained<NSURL>> = msg_send![item, infoURL];
                let size: u64 = msg_send![item, contentLength];
                let critical: bool = msg_send![item, isCriticalUpdate];
                let stage: isize = msg_send![state, stage];
                let user: bool = msg_send![state, userInitiated];
                (
                    text(v),
                    notes.or(info).and_then(|u| u.absoluteString()).map(|s| s.to_string()),
                    size,
                    critical,
                    stage,
                    user,
                )
            };
            self.ivars().cancel.replace(None);
            self.ivars().found.replace(Some(reply.copy()));
            let _ = self.ivars().tx.send(Event::Found {
                version,
                notes_url,
                size,
                critical,
                stage: match stage {
                    1 => Stage::Downloaded,
                    2 => Stage::Installing,
                    _ => Stage::NotDownloaded,
                },
                user_initiated,
            });
        }

        #[unsafe(method(showUpdateReleaseNotesWithDownloadData:))]
        fn show_notes(&self, _data: &AnyObject) {
            // The app links to the release page instead of rendering the notes.
        }

        #[unsafe(method(showUpdateReleaseNotesFailedToDownloadWithError:))]
        fn show_notes_failed(&self, error: &NSError) {
            let _ = self.ivars().tx.send(Event::NotesFailed(error_text(error)));
        }

        #[unsafe(method(showUpdateNotFoundWithError:acknowledgement:))]
        fn show_not_found(&self, error: &NSError, acknowledgement: &Block<dyn Fn()>) {
            self.ivars().cancel.replace(None);
            let _ = self.ivars().tx.send(Event::NotFound(error_text(error)));
            acknowledgement.call(());
        }

        #[unsafe(method(showUpdaterError:acknowledgement:))]
        fn show_error(&self, error: &NSError, acknowledgement: &Block<dyn Fn()>) {
            self.ivars().cancel.replace(None);
            let _ = self.ivars().tx.send(Event::Error(error_text(error)));
            acknowledgement.call(());
        }

        #[unsafe(method(showDownloadInitiatedWithCancellation:))]
        fn show_download(&self, cancellation: &Block<dyn Fn()>) {
            self.ivars().cancel.replace(Some(cancellation.copy()));
            let _ = self.ivars().tx.send(Event::DownloadStarted);
        }

        #[unsafe(method(showDownloadDidReceiveExpectedContentLength:))]
        fn show_total(&self, length: u64) {
            let _ = self.ivars().tx.send(Event::DownloadTotal(length));
        }

        #[unsafe(method(showDownloadDidReceiveDataOfLength:))]
        fn show_progress(&self, length: u64) {
            let _ = self.ivars().tx.send(Event::DownloadProgress(length));
        }

        #[unsafe(method(showDownloadDidStartExtractingUpdate))]
        fn show_extracting(&self) {
            self.ivars().cancel.replace(None);
            let _ = self.ivars().tx.send(Event::Extracting(0.0));
        }

        #[unsafe(method(showExtractionReceivedProgress:))]
        fn show_extraction(&self, progress: f64) {
            let _ = self.ivars().tx.send(Event::Extracting(progress));
        }

        #[unsafe(method(showReadyToInstallAndRelaunch:))]
        fn show_ready(&self, reply: &Block<dyn Fn(isize)>) {
            self.ivars().ready.replace(Some(reply.copy()));
            let _ = self.ivars().tx.send(Event::ReadyToInstall);
        }

        #[unsafe(method(showInstallingUpdateWithApplicationTerminated:retryTerminatingApplication:))]
        fn show_installing(&self, terminated: bool, retry: &Block<dyn Fn()>) {
            self.ivars().retry.replace(Some(retry.copy()));
            let _ = self.ivars().tx.send(Event::Installing {
                app_terminated: terminated,
            });
        }

        #[unsafe(method(showUpdateInstalledAndRelaunched:acknowledgement:))]
        fn show_installed(&self, relaunched: bool, acknowledgement: &Block<dyn Fn()>) {
            let _ = self.ivars().tx.send(Event::InstalledAndRelaunched(relaunched));
            acknowledgement.call(());
        }

        #[unsafe(method(showUpdateInFocus))]
        fn show_in_focus(&self) {}

        #[unsafe(method(dismissUpdateInstallation))]
        fn dismiss(&self) {
            self.ivars().found.replace(None);
            self.ivars().ready.replace(None);
            self.ivars().cancel.replace(None);
            self.ivars().retry.replace(None);
            let _ = self.ivars().tx.send(Event::Dismissed);
        }
    }
);

impl Driver {
    fn new(mtm: MainThreadMarker, tx: Sender<Event>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DriverIvars {
            tx,
            found: RefCell::new(None),
            ready: RefCell::new(None),
            cancel: RefCell::new(None),
            retry: RefCell::new(None),
        });
        // SAFETY: plain NSObject init.
        unsafe { msg_send![super(this), init] }
    }
}

struct DelegateIvars {
    feed: Option<String>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the ivars are initialised in `new`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZnimokSparkleDelegate"]
    #[ivars = DelegateIvars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    // SPUUpdaterDelegate (the parts used).
    impl Delegate {
        #[unsafe(method_id(feedURLStringForUpdater:))]
        fn feed_url(&self, _updater: &AnyObject) -> Option<Retained<NSString>> {
            self.ivars().feed.as_deref().map(NSString::from_str)
        }

        #[unsafe(method(updaterShouldPromptForPermissionToCheckForUpdates:))]
        fn should_prompt(&self, _updater: &AnyObject) -> bool {
            false
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker, feed: Option<String>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars { feed });
        // SAFETY: plain NSObject init.
        unsafe { msg_send![super(this), init] }
    }
}

/// A running `SPUUpdater` with Znimok's driver.
pub struct Sparkle {
    updater: Retained<AnyObject>,
    driver: Retained<Driver>,
    _delegate: Retained<Delegate>,
    rx: Receiver<Event>,
}

/// `Contents/Frameworks/Sparkle.framework` of the bundle this executable runs in.
pub fn framework_in_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let contents = exe.parent()?.parent()?;
    let fw = contents.join("Frameworks").join("Sparkle.framework");
    fw.join("Sparkle").exists().then_some(fw)
}

impl Sparkle {
    /// Load `Sparkle.framework` from `framework` and start an updater for `host` (the app
    /// bundle; `None` = the main bundle). `feed` overrides `SUFeedURL` of the bundle.
    pub fn start(
        framework: &Path,
        host: Option<&Path>,
        feed: Option<&str>,
    ) -> Result<Self, String> {
        let mtm = MainThreadMarker::new().ok_or("Sparkle: не головний потік")?;
        let bundle = NSBundle::bundleWithPath(&NSString::from_str(&framework.to_string_lossy()))
            .ok_or_else(|| format!("немає {}", framework.display()))?;
        // SAFETY: loading a framework bundle registers its classes; nothing else runs.
        if !unsafe { bundle.load() } {
            return Err(format!("не завантажився {}", framework.display()));
        }
        let cls = AnyClass::get(c"SPUUpdater").ok_or("у фреймворку немає SPUUpdater")?;
        let host = match host {
            Some(p) => NSBundle::bundleWithPath(&NSString::from_str(&p.to_string_lossy()))
                .ok_or_else(|| format!("немає бандла {}", p.display()))?,
            None => NSBundle::mainBundle(),
        };
        let (tx, rx) = channel();
        let driver = Driver::new(mtm, tx);
        let delegate = Delegate::new(mtm, feed.map(str::to_string));
        // SAFETY: the documented initialiser; the driver and delegate outlive the updater (kept
        // in `Sparkle`).
        let updater: Retained<AnyObject> = unsafe {
            let a: Allocated<AnyObject> = msg_send![cls, alloc];
            msg_send![
                a,
                initWithHostBundle: &*host,
                applicationBundle: &*host,
                userDriver: &*driver,
                delegate: &*delegate
            ]
        };
        // SAFETY: documented setters and the starting call with its error out-parameter.
        unsafe {
            let _: () = msg_send![&*updater, setAutomaticallyChecksForUpdates: false];
            let _: () = msg_send![&*updater, setAutomaticallyDownloadsUpdates: false];
            let r: Result<(), Retained<NSError>> = msg_send![&*updater, startUpdater: _];
            r.map_err(|e| format!("Sparkle: {}", error_text(&e)))?;
        }
        Ok(Self {
            updater,
            driver,
            _delegate: delegate,
            rx,
        })
    }

    /// A check: started by the person (Sparkle reports «no update» too) or in the background
    /// (silent unless an update is there).
    pub fn check(&self, user_initiated: bool) {
        // SAFETY: documented methods of SPUUpdater.
        unsafe {
            if user_initiated {
                let _: () = msg_send![&*self.updater, checkForUpdates];
            } else {
                let _: () = msg_send![&*self.updater, checkForUpdatesInBackground];
            }
        }
    }

    pub fn can_check(&self) -> bool {
        // SAFETY: documented property.
        unsafe { msg_send![&*self.updater, canCheckForUpdates] }
    }

    pub fn session_in_progress(&self) -> bool {
        // SAFETY: documented property.
        unsafe { msg_send![&*self.updater, sessionInProgress] }
    }

    /// Everything Sparkle said since the last poll.
    pub fn poll(&self) -> Vec<Event> {
        self.rx.try_iter().collect()
    }

    /// Whether an answer is awaited: to «install this update?» or «relaunch now?».
    pub fn awaits_reply(&self) -> bool {
        let iv = self.driver.ivars();
        iv.found.borrow().is_some() || iv.ready.borrow().is_some()
    }

    /// Answer the pending question (the found update, else the ready-to-install one).
    pub fn reply(&self, choice: Choice) -> bool {
        let iv = self.driver.ivars();
        let block = iv.found.take().or_else(|| iv.ready.take());
        match block {
            Some(b) => {
                b.call((choice as isize,));
                true
            }
            None => false,
        }
    }

    /// Cancel a running check or download.
    pub fn cancel(&self) {
        if let Some(b) = self.driver.ivars().cancel.take() {
            b.call(());
        }
    }

    /// The installer waits for the app to quit: ask it to try again (after the app saved).
    pub fn retry_terminate(&self) {
        if let Some(b) = self.driver.ivars().retry.take() {
            b.call(());
        }
    }
}

/// The selectors Sparkle calls on a user driver (for the tests: every one must be there).
pub const DRIVER_SELECTORS: &[&str] = &[
    "showUpdatePermissionRequest:reply:",
    "showUserInitiatedUpdateCheckWithCancellation:",
    "showUpdateFoundWithAppcastItem:state:reply:",
    "showUpdateReleaseNotesWithDownloadData:",
    "showUpdateReleaseNotesFailedToDownloadWithError:",
    "showUpdateNotFoundWithError:acknowledgement:",
    "showUpdaterError:acknowledgement:",
    "showDownloadInitiatedWithCancellation:",
    "showDownloadDidReceiveExpectedContentLength:",
    "showDownloadDidReceiveDataOfLength:",
    "showDownloadDidStartExtractingUpdate",
    "showExtractionReceivedProgress:",
    "showReadyToInstallAndRelaunch:",
    "showInstallingUpdateWithApplicationTerminated:retryTerminatingApplication:",
    "showUpdateInstalledAndRelaunched:acknowledgement:",
    "dismissUpdateInstallation",
    "showUpdateInFocus",
];

/// Whether the driver class answers every selector of the protocol (a test on the main thread).
pub fn driver_responds_to_all(mtm: MainThreadMarker) -> Vec<&'static str> {
    let (tx, _rx) = channel();
    let d = Driver::new(mtm, tx);
    DRIVER_SELECTORS
        .iter()
        .copied()
        .filter(|s| {
            let sel = objc2::runtime::Sel::register(&std::ffi::CString::new(*s).unwrap());
            !d.respondsToSelector(sel)
        })
        .collect()
}
