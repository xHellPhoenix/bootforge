//! Windows-Antwortdatei (`autounattend.xml`): Fragen der Windows-Installation vorab beantworten.
//!
//! Windows-Setup sucht beim Start im Hauptverzeichnis aller Laufwerke nach `autounattend.xml`.
//! Die Datei ist in „Durchgänge“ (*passes*) gegliedert, die zu verschiedenen Zeitpunkten gelesen
//! werden:
//!
//! ```text
//! windowsPE   – Setup selbst (vor dem Kopieren): Sprache des Setups, Hardware-Prüfungen
//! specialize  – erster Start des neuen Windows: Computername, Netzwerk-Zwang, Datenschutz
//! oobeSystem  – Willkommens-Assistent (OOBE): Region, Tastatur, Konto, Datenschutz-Seiten
//! ```
//!
//! **Bewusst NICHT enthalten:** Festplatte partitionieren/löschen. Das bleibt eine Entscheidung
//! am Bildschirm. Ein Stick, der beim falschen PC automatisch die Platte löscht, wäre gefährlich.
//!
//! Sicherheit: Alle Eingaben werden geprüft (erlaubte Zeichen und Längen wie bei Windows) und
//! zusätzlich XML-maskiert. Die Datei enthält **kein Passwort**: Das lokale Konto wird ohne
//! Passwort angelegt, man setzt es nach der ersten Anmeldung in Windows.

use serde::{Deserialize, Serialize};

/// Was der Nutzer vor dem Erstellen des Sticks festgelegt hat. Alles optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WindowsSetup {
    /// Lokales Konto (statt Microsoft-Konto) mit diesem Benutzernamen anlegen, ohne Passwort.
    pub local_account: Option<String>,
    /// Computername (max. 15 Zeichen). Leer: Windows vergibt einen.
    pub computer_name: Option<String>,
    /// Region und Tastatur, z. B. „de-DE“. Leer: Windows fragt wie gewohnt.
    pub locale: Option<String>,
    /// Datenschutz-Fragen überspringen (alles auf „aus“).
    pub skip_privacy: bool,
    /// Prüfung auf TPM 2.0, Secure Boot und 4 GB RAM überspringen (nicht offiziell unterstützt).
    pub bypass_requirements: bool,
}

/// Von Windows reservierte Kontonamen.
const RESERVED_ACCOUNTS: &[&str] = &[
    "administrator",
    "admin",
    "guest",
    "gast",
    "defaultaccount",
    "wdagutilityaccount",
    "system",
    "none",
];

impl WindowsSetup {
    /// Nichts ausgewählt → keine Antwortdatei nötig.
    pub fn is_empty(&self) -> bool {
        self.normalized() == WindowsSetup::default()
    }

    /// Leere Textfelder wie „nicht gesetzt“ behandeln, Leerzeichen am Rand entfernen.
    fn normalized(&self) -> WindowsSetup {
        let clean = |s: &Option<String>| {
            s.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        WindowsSetup {
            local_account: clean(&self.local_account),
            computer_name: clean(&self.computer_name),
            locale: clean(&self.locale),
            skip_privacy: self.skip_privacy,
            bypass_requirements: self.bypass_requirements,
        }
    }

    /// Prüft alle Eingaben. Fehlertext ist für Menschen gedacht.
    pub fn validate(&self) -> Result<(), String> {
        let s = self.normalized();
        if let Some(name) = &s.local_account {
            // Windows: max. 20 Zeichen, keine dieser Zeichen, nicht nur Punkte/Leerzeichen.
            const BAD: &[char] = &[
                '"', '/', '\\', '[', ']', ':', ';', '|', '=', ',', '+', '*', '?', '<', '>', '@',
                '%',
            ];
            if name.chars().count() > 20 {
                return Err("Benutzername: höchstens 20 Zeichen".into());
            }
            if name.chars().any(|c| BAD.contains(&c) || c.is_control()) {
                return Err("Benutzername enthält ein unzulässiges Zeichen".into());
            }
            if name.chars().all(|c| c == '.' || c == ' ') {
                return Err("Benutzername darf nicht nur aus Punkten bestehen".into());
            }
            if RESERVED_ACCOUNTS.contains(&name.to_lowercase().as_str()) {
                return Err(format!("„{name}“ ist in Windows schon vergeben"));
            }
        }
        if let Some(pc) = &s.computer_name {
            // NetBIOS-Regel: 1–15 Zeichen, Buchstaben/Ziffern/Bindestrich, nicht nur Ziffern.
            let ok_chars = pc.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            if pc.len() > 15 || !ok_chars || pc.starts_with('-') || pc.ends_with('-') {
                return Err(
                    "Computername: 1–15 Zeichen, nur Buchstaben (ohne Umlaute), Ziffern und Bindestrich"
                        .into(),
                );
            }
            if pc.chars().all(|c| c.is_ascii_digit()) {
                return Err("Computername darf nicht nur aus Ziffern bestehen".into());
            }
        }
        if let Some(loc) = &s.locale
            && !is_locale_tag(loc)
        {
            return Err(format!(
                "unbekanntes Sprachformat „{loc}“ (erwartet z. B. de-DE)"
            ));
        }
        Ok(())
    }

    /// Erzeugt die Antwortdatei. `ui_language` ist die Sprache der ISO (aus `sources/lang.ini`);
    /// Windows bricht ab, wenn eine Sprache verlangt wird, die das Image nicht enthält.
    pub fn to_xml(&self, ui_language: Option<&str>) -> Result<String, String> {
        self.validate()?;
        let s = self.normalized();
        let ui = ui_language.filter(|l| is_locale_tag(l));
        let mut x = String::new();
        x.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
        x.push_str("<!-- Erstellt von SealFlash. Enthaelt keine Passwoerter und partitioniert nichts automatisch. -->\n");
        x.push_str("<unattend xmlns=\"urn:schemas-microsoft-com:unattend\" xmlns:wcm=\"http://schemas.microsoft.com/WMIConfig/2002/State\">\n");

        // --- windowsPE -------------------------------------------------------------------------
        let mut pe = String::new();
        if let (Some(loc), Some(ui)) = (&s.locale, ui) {
            pe += &component(
                "Microsoft-Windows-International-Core-WinPE",
                &format!(
                    "<SetupUILanguage><UILanguage>{ui}</UILanguage></SetupUILanguage>\
                     <InputLocale>{l}</InputLocale><SystemLocale>{l}</SystemLocale>\
                     <UILanguage>{ui}</UILanguage><UserLocale>{l}</UserLocale>",
                    ui = esc(ui),
                    l = esc(loc)
                ),
            );
        }
        if s.bypass_requirements {
            // Windows 11 prüft TPM/Secure Boot/RAM im Setup. Diese Registry-Werte (von Microsoft
            // für Testzwecke vorgesehen, „LabConfig“) schalten die Prüfung ab.
            let cmds: Vec<String> = ["BypassTPMCheck", "BypassSecureBootCheck", "BypassRAMCheck"]
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    sync_command(
                        i + 1,
                        &format!(
                            "reg add HKLM\\SYSTEM\\Setup\\LabConfig /v {v} /t REG_DWORD /d 1 /f"
                        ),
                    )
                })
                .collect();
            pe += &component(
                "Microsoft-Windows-Setup",
                &format!("<RunSynchronous>{}</RunSynchronous>", cmds.concat()),
            );
        }
        x += &pass("windowsPE", &pe);

        // --- specialize ------------------------------------------------------------------------
        let mut sp = String::new();
        if let Some(pc) = &s.computer_name {
            sp += &component(
                "Microsoft-Windows-Shell-Setup",
                &format!("<ComputerName>{}</ComputerName>", esc(pc)),
            );
        }
        let mut cmds = Vec::new();
        if s.local_account.is_some() {
            // Ohne Internet weitermachen dürfen (sonst verlangt Windows 11 ein Microsoft-Konto).
            cmds.push("reg add HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\OOBE /v BypassNRO /t REG_DWORD /d 1 /f");
        }
        if s.skip_privacy {
            cmds.push("reg add HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\OOBE /v DisablePrivacyExperience /t REG_DWORD /d 1 /f");
        }
        if !cmds.is_empty() {
            let list: Vec<String> = cmds
                .iter()
                .enumerate()
                .map(|(i, c)| sync_command(i + 1, c))
                .collect();
            sp += &component(
                "Microsoft-Windows-Deployment",
                &format!("<RunSynchronous>{}</RunSynchronous>", list.concat()),
            );
        }
        x += &pass("specialize", &sp);

        // --- oobeSystem ------------------------------------------------------------------------
        let mut oobe = String::new();
        if let Some(loc) = &s.locale {
            let ui_part = ui
                .map(|u| format!("<UILanguage>{}</UILanguage>", esc(u)))
                .unwrap_or_default();
            oobe += &component(
                "Microsoft-Windows-International-Core",
                &format!(
                    "<InputLocale>{l}</InputLocale><SystemLocale>{l}</SystemLocale>{ui_part}<UserLocale>{l}</UserLocale>",
                    l = esc(loc)
                ),
            );
        }
        let mut shell = String::new();
        if s.local_account.is_some() || s.skip_privacy {
            let mut o = String::new();
            if s.local_account.is_some() {
                o += "<HideOnlineAccountScreens>true</HideOnlineAccountScreens>";
            }
            if s.skip_privacy {
                // 3 = keine „Express-Einstellungen“, d. h. Datenübermittlung aus.
                o += "<ProtectYourPC>3</ProtectYourPC>";
            }
            shell += &format!("<OOBE>{o}</OOBE>");
        }
        if let Some(name) = &s.local_account {
            shell += &format!(
                "<UserAccounts><LocalAccounts>\
                 <LocalAccount wcm:action=\"add\">\
                 <Name>{n}</Name><DisplayName>{n}</DisplayName><Group>Administrators</Group>\
                 <Password><Value></Value><PlainText>true</PlainText></Password>\
                 </LocalAccount></LocalAccounts></UserAccounts>",
                n = esc(name)
            );
        }
        if !shell.is_empty() {
            oobe += &component("Microsoft-Windows-Shell-Setup", &shell);
        }
        x += &pass("oobeSystem", &oobe);

        x.push_str("</unattend>\n");
        Ok(x)
    }
}

/// „de-DE“, „en-US“, „sr-Latn-RS“ … (Buchstaben/Ziffern, durch Bindestrich getrennt).
fn is_locale_tag(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    s.len() <= 20
        && parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 8 && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// XML-Sonderzeichen maskieren.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn pass(name: &str, body: &str) -> String {
    if body.is_empty() {
        String::new()
    } else {
        format!("  <settings pass=\"{name}\">\n{body}  </settings>\n")
    }
}

fn component(name: &str, body: &str) -> String {
    format!(
        "    <component name=\"{name}\" processorArchitecture=\"amd64\" publicKeyToken=\"31bf3856ad364e35\" language=\"neutral\" versionScope=\"nonSxS\">{body}</component>\n"
    )
}

fn sync_command(order: usize, cmd: &str) -> String {
    format!(
        "<RunSynchronousCommand wcm:action=\"add\"><Order>{order}</Order><Path>{}</Path></RunSynchronousCommand>",
        esc(cmd)
    )
}

/// Sprache der ISO aus `sources/lang.ini` lesen. Inhalt z. B.:
///
/// ```text
/// [Available UI Languages]
/// de-DE = 3
/// [Fallback Languages]
/// de-DE = en-us
/// ```
pub fn ui_language_from_lang_ini(text: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.starts_with('[') {
            in_section = line.eq_ignore_ascii_case("[Available UI Languages]");
            continue;
        }
        if in_section && let Some((lang, _)) = line.split_once('=') {
            let lang = lang.trim();
            if is_locale_tag(lang) {
                return Some(lang.to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> WindowsSetup {
        WindowsSetup {
            local_account: Some("Alex".into()),
            computer_name: Some("WERKSTATT-PC1".into()),
            locale: Some("de-DE".into()),
            skip_privacy: true,
            bypass_requirements: true,
        }
    }

    #[test]
    fn empty_means_no_file() {
        assert!(WindowsSetup::default().is_empty());
        let only_spaces = WindowsSetup {
            local_account: Some("   ".into()),
            ..Default::default()
        };
        assert!(only_spaces.is_empty());
        assert!(!full().is_empty());
    }

    #[test]
    fn rejects_bad_input() {
        let bad = |s: WindowsSetup| assert!(s.validate().is_err(), "{s:?}");
        bad(WindowsSetup {
            local_account: Some("a<b".into()),
            ..Default::default()
        });
        bad(WindowsSetup {
            local_account: Some("Administrator".into()),
            ..Default::default()
        });
        bad(WindowsSetup {
            local_account: Some("x".repeat(21)),
            ..Default::default()
        });
        bad(WindowsSetup {
            computer_name: Some("Büro".into()),
            ..Default::default()
        });
        bad(WindowsSetup {
            computer_name: Some("12345".into()),
            ..Default::default()
        });
        bad(WindowsSetup {
            computer_name: Some("a".repeat(16)),
            ..Default::default()
        });
        bad(WindowsSetup {
            locale: Some("de_DE\"><x".into()),
            ..Default::default()
        });
        // Umlaute im Benutzernamen sind in Windows erlaubt.
        assert!(
            WindowsSetup {
                local_account: Some("Jürgen Müller".into()),
                ..Default::default()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn xml_contains_choices_and_no_password_or_disk_config() {
        let x = full().to_xml(Some("de-DE")).unwrap();
        for want in [
            "<Name>Alex</Name>",
            "<ComputerName>WERKSTATT-PC1</ComputerName>",
            "<InputLocale>de-DE</InputLocale>",
            "<ProtectYourPC>3</ProtectYourPC>",
            "BypassTPMCheck",
            "BypassNRO",
            "<Value></Value>",
            "<HideOnlineAccountScreens>true</HideOnlineAccountScreens>",
        ] {
            assert!(x.contains(want), "fehlt: {want}");
        }
        assert!(
            !x.contains("DiskConfiguration"),
            "niemals automatisch partitionieren"
        );
        assert!(!x.contains("ProductKey"));
    }

    #[test]
    fn locale_without_iso_language_skips_setup_language() {
        let s = WindowsSetup {
            locale: Some("de-DE".into()),
            ..Default::default()
        };
        let x = s.to_xml(None).unwrap();
        assert!(
            !x.contains("UILanguage"),
            "keine Sprache erzwingen, die die ISO evtl. nicht hat"
        );
        assert!(x.contains("<UserLocale>de-DE</UserLocale>"));
        assert!(!x.contains("windowsPE"));
    }

    #[test]
    fn umlauts_are_valid_utf8_xml() {
        let s = WindowsSetup {
            local_account: Some("Jürgen & Co".into()),
            ..Default::default()
        };
        let x = s.to_xml(None).unwrap();
        assert!(x.contains("<Name>Jürgen &amp; Co</Name>"));
    }

    /// Wohlgeformtes XML? (Windows-Setup bricht sonst mit „Antwortdatei ungültig“ ab.)
    #[test]
    fn xml_is_well_formed() {
        for s in [
            full(),
            WindowsSetup::default(),
            WindowsSetup {
                local_account: Some("Jürgen & Co".into()),
                ..Default::default()
            },
        ] {
            let x = s.to_xml(Some("de-DE")).unwrap();
            let mut r = quick_xml::Reader::from_str(&x);
            r.config_mut().check_end_names = true;
            let mut depth = 0i32;
            loop {
                match r.read_event().unwrap_or_else(|e| panic!("{e}\n{x}")) {
                    quick_xml::events::Event::Start(_) => depth += 1,
                    quick_xml::events::Event::End(_) => depth -= 1,
                    quick_xml::events::Event::Eof => break,
                    _ => {}
                }
            }
            assert_eq!(depth, 0);
        }
    }

    #[test]
    fn reads_iso_language() {
        let ini = "\u{feff}[Available UI Languages]\r\nde-DE = 3\r\n\r\n[Fallback Languages]\r\nde-DE = en-us\r\n";
        assert_eq!(ui_language_from_lang_ini(ini).as_deref(), Some("de-DE"));
        assert_eq!(
            ui_language_from_lang_ini("[Fallback Languages]\nde-DE = en-us"),
            None
        );
    }
}
