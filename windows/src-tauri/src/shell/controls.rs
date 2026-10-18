// The Controls tab (docs/WINDOWS_SHELL.md): volume and mute, Wi-Fi and
// Bluetooth, the brightness of screens that accept it, and links to the
// Settings pages for what Windows gives no API for (picking the audio output,
// the theme, night light).

// The pure helpers here serve the Windows code; elsewhere only the tests use them.
#![cfg_attr(not(windows), allow(dead_code))]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Controls {
    /// 0–1, None where there is no output device.
    pub volume: Option<f32>,
    pub muted: Option<bool>,
    /// The default output's name ("Casque (Realtek Audio)").
    pub output: Option<String>,
    pub radios: Vec<Radio>,
    pub displays: Vec<Display>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Radio {
    /// "wifi", "bluetooth", "mobile" or "other".
    pub kind: String,
    pub name: String,
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Display {
    pub index: usize,
    pub name: String,
    /// 0–100; None when the screen does not take DDC/CI.
    pub brightness: Option<u32>,
}

/// The Settings pages the panel may open: a fixed list, never a URI from the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingsPage {
    Sound,
    Colors,
    NightLight,
    Display,
    Bluetooth,
    Network,
}

impl SettingsPage {
    pub fn uri(self) -> &'static str {
        match self {
            SettingsPage::Sound => "ms-settings:sound",
            SettingsPage::Colors => "ms-settings:colors",
            SettingsPage::NightLight => "ms-settings:nightlight",
            SettingsPage::Display => "ms-settings:display",
            SettingsPage::Bluetooth => "ms-settings:bluetooth",
            SettingsPage::Network => "ms-settings:network",
        }
    }
}

/// A volume as the endpoint takes it: finite, 0–1.
pub fn clamp_volume(v: f32) -> Option<f32> {
    v.is_finite().then(|| v.clamp(0.0, 1.0))
}

#[cfg(windows)]
pub fn read() -> Result<Controls, String> {
    Ok(super::win::read_controls())
}

#[cfg(windows)]
pub fn set_volume(level: f32) -> Result<(), String> {
    super::win::set_volume(clamp_volume(level).ok_or("bad volume")?)
}

#[cfg(windows)]
pub fn set_mute(muted: bool) -> Result<(), String> {
    super::win::set_mute(muted)
}

#[cfg(windows)]
pub fn set_radio(kind: &str, on: bool) -> Result<(), String> {
    super::win::set_radio(kind, on)
}

#[cfg(windows)]
pub fn set_brightness(index: usize, value: u32) -> Result<(), String> {
    super::win::set_brightness(index, value.min(100))
}

#[cfg(windows)]
pub fn open_settings(page: SettingsPage) -> Result<(), String> {
    super::win::shell_open(page.uri())
}

#[cfg(not(windows))]
mod unsupported {
    use super::*;
    fn no<T>() -> Result<T, String> {
        Err(crate::i18n::t("Not available on this system."))
    }
    pub fn read() -> Result<Controls, String> {
        no()
    }
    pub fn set_volume(_level: f32) -> Result<(), String> {
        no()
    }
    pub fn set_mute(_muted: bool) -> Result<(), String> {
        no()
    }
    pub fn set_radio(_kind: &str, _on: bool) -> Result<(), String> {
        no()
    }
    pub fn set_brightness(_index: usize, _value: u32) -> Result<(), String> {
        no()
    }
    pub fn open_settings(_page: SettingsPage) -> Result<(), String> {
        no()
    }
}
#[cfg(not(windows))]
pub use unsupported::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_pages_are_a_fixed_list() {
        let page: SettingsPage = serde_json::from_str("\"nightLight\"").unwrap();
        assert_eq!(page.uri(), "ms-settings:nightlight");
        assert!(serde_json::from_str::<SettingsPage>("\"ms-settings:privacy\"").is_err());
        assert!(serde_json::from_str::<SettingsPage>("\"file:///C:/x\"").is_err());
    }

    #[test]
    fn volumes_are_clamped_and_never_nan() {
        assert_eq!(clamp_volume(1.5), Some(1.0));
        assert_eq!(clamp_volume(-1.0), Some(0.0));
        assert_eq!(clamp_volume(f32::NAN), None);
    }
}
