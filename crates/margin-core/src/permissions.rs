//! macOS privacy permissions Margin needs, with status checks, native prompts and deep links
//! straight to the right System Settings pane.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermStatus {
    NotDetermined,
    Denied,
    Restricted,
    Granted,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Microphone,
    SystemAudio,
    Calendar,
}

impl Permission {
    /// Deep link to the matching Privacy & Security pane.
    pub fn settings_url(self) -> &'static str {
        match self {
            Permission::Microphone => "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
            Permission::SystemAudio => "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
            Permission::Calendar => "x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars",
        }
    }
}

pub fn status(p: Permission) -> PermStatus {
    match p {
        Permission::Microphone => mic_status(),
        Permission::SystemAudio => {
            if crate::audio::system_audio_permitted() {
                PermStatus::Granted
            } else {
                // macOS can't tell "never asked" from "denied" for screen capture.
                PermStatus::NotDetermined
            }
        }
        Permission::Calendar => match crate::calendar::auth_status() {
            crate::calendar::CalendarAuth::Authorized => PermStatus::Granted,
            crate::calendar::CalendarAuth::Denied => PermStatus::Denied,
            crate::calendar::CalendarAuth::Restricted => PermStatus::Restricted,
            crate::calendar::CalendarAuth::NotDetermined => PermStatus::NotDetermined,
            crate::calendar::CalendarAuth::Unsupported => PermStatus::Unsupported,
        },
    }
}

/// Show the native prompt if macOS still allows it; returns the status afterwards.
/// Blocks until the user answers (call from a worker thread).
pub fn request(p: Permission) -> PermStatus {
    match p {
        Permission::Microphone => {
            if mic_status() == PermStatus::NotDetermined {
                request_mic();
            }
        }
        Permission::SystemAudio => {
            crate::audio::request_system_audio_permission();
        }
        Permission::Calendar => {
            let _ = crate::calendar::request_access();
        }
    }
    status(p)
}

/// Open System Settings at the right pane.
pub fn open_settings(p: Permission) -> std::io::Result<()> {
    std::process::Command::new("/usr/bin/open").arg(p.settings_url()).spawn().map(|_| ())
}

#[cfg(target_os = "macos")]
mod mac {
    use super::PermStatus;
    use block2::RcBlock;
    use objc2::runtime::{AnyClass, Bool};
    use objc2::msg_send;
    use objc2_foundation::NSString;

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {}

    fn av_capture_device() -> Option<&'static AnyClass> {
        AnyClass::get(c"AVCaptureDevice")
    }

    /// AVMediaTypeAudio is the four-char string "soun".
    fn media_audio() -> objc2::rc::Retained<NSString> {
        NSString::from_str("soun")
    }

    pub fn mic_status() -> PermStatus {
        let Some(cls) = av_capture_device() else { return PermStatus::Unsupported };
        let media = media_audio();
        // SAFETY: class method with an NSString argument returning NSInteger.
        let s: isize = unsafe { msg_send![cls, authorizationStatusForMediaType: &*media] };
        match s {
            0 => PermStatus::NotDetermined,
            1 => PermStatus::Restricted,
            2 => PermStatus::Denied,
            3 => PermStatus::Granted,
            _ => PermStatus::Unsupported,
        }
    }

    pub fn request_mic() {
        let Some(cls) = av_capture_device() else { return };
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        let block = RcBlock::new(move |granted: Bool| {
            let _ = tx.send(granted.as_bool());
        });
        let media = media_audio();
        // SAFETY: the completion block is retained by AVFoundation until it fires.
        let _: () = unsafe { msg_send![cls, requestAccessForMediaType: &*media, completionHandler: &*block] };
        let _ = rx.recv_timeout(std::time::Duration::from_secs(120));
    }
}

#[cfg(target_os = "macos")]
use mac::{mic_status, request_mic};

#[cfg(not(target_os = "macos"))]
fn mic_status() -> PermStatus {
    PermStatus::Unsupported
}
#[cfg(not(target_os = "macos"))]
fn request_mic() {}
