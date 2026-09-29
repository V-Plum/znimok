//! `.znimok` files open in Znimok (ZK-75): per-user registration in `HKCU\Software\Classes`,
//! no admin rights, removed cleanly on uninstall.
//!
//! - `.znimok` → ProgID `Znimok.Document` (plus `OpenWithProgids`, so Znimok stays in «Open
//!   with» even if the user picks another program);
//! - `Znimok.Document`: type name, icon, `shell\open\command` = `"<exe>" "%1"`;
//! - `Applications\<exe name>\SupportedTypes` — the app in «Open with» for the type.
//!
//! Windows 8+ keeps the user's choice in `…\Explorer\FileExts\.znimok\UserChoice` (hash-protected;
//! programs must not write it). [`WinFileAssoc::state`] reads it: if the user chose another
//! program, that is reported and respected — the settings page can only suggest «Open with».

use std::path::Path;

use super::reg::{delete_tree, delete_value, read_sz, read_sz_in, write_sz};
use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
use znimok_platform::{AssocState, FileAssoc, PlatformError, Result};

pub const PROG_ID: &str = "Znimok.Document";

pub struct WinFileAssoc {
    classes: String,
    file_exts: String,
    prog_id: String,
    exe: String,
    type_name: String,
    icon: String,
    notify: bool,
}

impl WinFileAssoc {
    /// `exe` opens the files; `type_name` is shown in Explorer («Знімок Znimok»); `icon` is
    /// `path,index` (the exe's document icon — the installer puts it there).
    pub fn new(exe: &Path, type_name: &str, icon: Option<&str>) -> Self {
        let exe = exe.display().to_string();
        Self {
            classes: r"Software\Classes".into(),
            file_exts: r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts".into(),
            prog_id: PROG_ID.into(),
            icon: icon
                .map(str::to_string)
                .unwrap_or_else(|| format!("{exe},0")),
            exe,
            type_name: type_name.into(),
            notify: true,
        }
    }

    /// Other roots (tests use a throw-away key instead of the real `Software\Classes`).
    pub fn with_roots(mut self, classes: &str, file_exts: &str) -> Self {
        self.classes = classes.into();
        self.file_exts = file_exts.into();
        self.notify = false;
        self
    }

    fn ext_key(&self, ext: &str) -> String {
        format!(r"{}\.{}", self.classes, ext.trim_start_matches('.'))
    }

    fn exe_name(&self) -> String {
        Path::new(&self.exe)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "znimok-app.exe".into())
    }

    fn changed(&self) {
        if self.notify {
            // SAFETY: documented «associations changed» broadcast, no items.
            unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
        }
    }
}

impl FileAssoc for WinFileAssoc {
    fn state(&self, ext: &str) -> Result<AssocState> {
        let ext = ext.trim_start_matches('.');
        let user = format!(r"{}\.{ext}\UserChoice", self.file_exts);
        let choice = read_sz(&user, "ProgId")?.filter(|p| !p.is_empty());
        let registered = read_sz(&self.ext_key(ext), "")?.filter(|p| !p.is_empty());
        let machine = if self.notify {
            read_sz_in(HKEY_LOCAL_MACHINE, &format!(r"Software\Classes\.{ext}"), "")?
                .filter(|p| !p.is_empty())
        } else {
            None
        };
        Ok(match choice.or(registered).or(machine) {
            Some(p) if p.eq_ignore_ascii_case(&self.prog_id) => AssocState::Ours,
            // A choice made in «Open with» for the exe itself.
            Some(p) if p.eq_ignore_ascii_case(&format!("Applications\\{}", self.exe_name())) => {
                AssocState::Ours
            }
            Some(p) => AssocState::Other(p),
            None => AssocState::None,
        })
    }

    fn register(&self, ext: &str) -> Result<()> {
        let ext = ext.trim_start_matches('.');
        if ext.is_empty() || ext.contains(['\\', '/']) {
            return Err(PlatformError::Other(format!("bad extension «{ext}»")));
        }
        let prog = format!(r"{}\{}", self.classes, self.prog_id);
        write_sz(&prog, "", &self.type_name)?;
        write_sz(&prog, "FriendlyTypeName", &self.type_name)?;
        // With the thumbnail DLL's icon handler registered, the type's icon is "%1" (asked per
        // file: screenshot, video, video with a log — ZK-150); keep it.
        let per_file = read_sz(&format!(r"{prog}\shellex\IconHandler"), "")?.is_some()
            && read_sz(&format!(r"{prog}\DefaultIcon"), "")?.as_deref() == Some("%1");
        if !per_file {
            write_sz(&format!(r"{prog}\DefaultIcon"), "", &self.icon)?;
        }
        write_sz(
            &format!(r"{prog}\shell\open\command"),
            "",
            &format!("\"{}\" \"%1\"", self.exe),
        )?;
        let ext_key = self.ext_key(ext);
        // Do not take over a type another program already owns by default.
        match read_sz(&ext_key, "")?.filter(|p| !p.is_empty()) {
            None => write_sz(&ext_key, "", &self.prog_id)?,
            Some(p) if p == self.prog_id => {}
            Some(_) => {}
        }
        write_sz(&format!(r"{ext_key}\OpenWithProgids"), &self.prog_id, "")?;
        let app = format!(r"{}\Applications\{}", self.classes, self.exe_name());
        write_sz(&format!(r"{app}\SupportedTypes"), &format!(".{ext}"), "")?;
        write_sz(
            &format!(r"{app}\shell\open\command"),
            "",
            &format!("\"{}\" \"%1\"", self.exe),
        )?;
        self.changed();
        Ok(())
    }

    fn unregister(&self, ext: &str) -> Result<()> {
        let ext = ext.trim_start_matches('.');
        let ext_key = self.ext_key(ext);
        if read_sz(&ext_key, "")?.as_deref() == Some(self.prog_id.as_str()) {
            delete_value(&ext_key, "")?;
        }
        delete_value(&format!(r"{ext_key}\OpenWithProgids"), &self.prog_id)?;
        delete_tree(&format!(r"{}\{}", self.classes, self.prog_id))?;
        delete_tree(&format!(
            r"{}\Applications\{}",
            self.classes,
            self.exe_name()
        ))?;
        self.changed();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(String);
    impl Scratch {
        fn new(tag: &str) -> Self {
            Self(format!(
                r"Software\ZnimokTest-assoc-{tag}-{}",
                std::process::id()
            ))
        }
        fn assoc(&self) -> WinFileAssoc {
            WinFileAssoc::new(
                Path::new(r"C:\Users\u\AppData\Local\Programs\Znimok\znimok-app.exe"),
                "Знімок Znimok",
                None,
            )
            .with_roots(
                &format!(r"{}\Classes", self.0),
                &format!(r"{}\FileExts", self.0),
            )
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = delete_tree(&self.0);
        }
    }

    /// With the icon handler in place (ZK-150), registering again keeps the per-file icon.
    #[test]
    fn per_file_icon_is_kept() {
        let s = Scratch::new("icon");
        let a = s.assoc();
        a.register("znimok").unwrap();
        let prog = format!(r"{}\Classes\Znimok.Document", s.0);
        let icon = || read_sz(&format!(r"{prog}\DefaultIcon"), "").unwrap();
        assert_ne!(
            icon().as_deref(),
            Some("%1"),
            "without the handler: the app's icon"
        );
        write_sz(
            &format!(r"{prog}\shellex\IconHandler"),
            "",
            "{0F5C6E12-2A39-4AAC-9C34-3E2C9305E05A}",
        )
        .unwrap();
        write_sz(&format!(r"{prog}\DefaultIcon"), "", "%1").unwrap();
        a.register("znimok").unwrap();
        assert_eq!(icon().as_deref(), Some("%1"));
    }

    #[test]
    fn register_state_unregister() {
        let s = Scratch::new("cycle");
        let a = s.assoc();
        assert_eq!(a.state("znimok").unwrap(), AssocState::None);
        a.register("znimok").unwrap();
        assert_eq!(a.state(".znimok").unwrap(), AssocState::Ours);
        let classes = format!(r"{}\Classes", s.0);
        assert_eq!(
            read_sz(
                &format!(r"{classes}\Znimok.Document\shell\open\command"),
                ""
            )
            .unwrap()
            .as_deref(),
            Some(r#""C:\Users\u\AppData\Local\Programs\Znimok\znimok-app.exe" "%1""#)
        );
        assert_eq!(
            read_sz(&format!(r"{classes}\Znimok.Document"), "")
                .unwrap()
                .as_deref(),
            Some("Знімок Znimok")
        );
        a.register("znimok").unwrap(); // twice is fine
        a.unregister("znimok").unwrap();
        assert_eq!(a.state("znimok").unwrap(), AssocState::None);
        assert_eq!(
            read_sz(&format!(r"{classes}\Znimok.Document"), "").unwrap(),
            None
        );
    }

    #[test]
    fn the_users_choice_wins_and_another_owner_is_not_overwritten() {
        let s = Scratch::new("choice");
        let a = s.assoc();
        let classes = format!(r"{}\Classes", s.0);
        write_sz(&format!(r"{classes}\.znimok"), "", "Other.App").unwrap();
        a.register("znimok").unwrap();
        // The default stays with the other program; we are in «Open with».
        assert_eq!(
            read_sz(&format!(r"{classes}\.znimok"), "")
                .unwrap()
                .as_deref(),
            Some("Other.App")
        );
        assert_eq!(
            a.state("znimok").unwrap(),
            AssocState::Other("Other.App".into())
        );
        // The user picks Znimok in «Open with» → Windows writes UserChoice.
        write_sz(
            &format!(r"{}\FileExts\.znimok\UserChoice", s.0),
            "ProgId",
            PROG_ID,
        )
        .unwrap();
        assert_eq!(a.state("znimok").unwrap(), AssocState::Ours);
        a.unregister("znimok").unwrap();
        assert_eq!(
            read_sz(&format!(r"{classes}\.znimok"), "")
                .unwrap()
                .as_deref(),
            Some("Other.App"),
            "someone else's default is left alone"
        );
    }

    #[test]
    fn bad_extension() {
        let s = Scratch::new("bad");
        assert!(s.assoc().register(r"a\b").is_err());
    }
}
