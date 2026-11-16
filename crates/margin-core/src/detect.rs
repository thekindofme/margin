//! Meeting detection without calendar: is another app using the mic, and which meeting apps run.
//! OWNER: calendar worker.

/// True when any process is using the default input device (CoreAudio
/// kAudioDevicePropertyDeviceIsRunningSomewhere). Includes us when we are recording.
pub fn mic_in_use() -> bool {
    imp::mic_in_use()
}

/// Display names of known meeting apps that are currently running (Zoom, Teams, Webex, Slack, FaceTime, Discord...).
pub fn running_meeting_apps() -> Vec<String> {
    imp::running_meeting_apps()
}

/// If the mic is in use and a known meeting app is running, the name of the first such app.
pub fn meeting_app_using_mic() -> Option<String> {
    if !mic_in_use() {
        return None;
    }
    running_meeting_apps().into_iter().next()
}

/// Map a bundle identifier to a meeting app display name.
pub fn meeting_app_name(bundle_id: &str) -> Option<&'static str> {
    Some(match bundle_id {
        "us.zoom.xos" => "Zoom",
        "com.microsoft.teams2" | "com.microsoft.teams" => "Microsoft Teams",
        "com.cisco.webexmeetingsapp" | "Cisco-Systems.Spark" => "Webex",
        "com.tinyspeck.slackmacgap" => "Slack",
        "com.apple.FaceTime" => "FaceTime",
        "com.hnc.Discord" => "Discord",
        _ => return None,
    })
}

#[cfg(target_os = "macos")]
mod imp {
    use super::meeting_app_name;
    use objc2_app_kit::NSWorkspace;
    use std::ffi::c_void;

    #[repr(C)]
    struct AudioObjectPropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyData(
            object: u32,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            data_size: *mut u32,
            data: *mut c_void,
        ) -> i32;
    }

    const SYSTEM_OBJECT: u32 = 1;
    const DEFAULT_INPUT_DEVICE: u32 = 0x6449_6E20; // 'dIn '
    const IS_RUNNING_SOMEWHERE: u32 = 0x676F_6E65; // 'gone'
    const SCOPE_GLOBAL: u32 = 0x676C_6F62; // 'glob'
    const ELEMENT_MAIN: u32 = 0;

    /// Reads a single u32 property; None on any CoreAudio error.
    fn get_u32(object: u32, selector: u32) -> Option<u32> {
        let addr = AudioObjectPropertyAddress {
            selector,
            scope: SCOPE_GLOBAL,
            element: ELEMENT_MAIN,
        };
        let mut value: u32 = 0;
        let mut size: u32 = std::mem::size_of::<u32>() as u32;
        // SAFETY: `addr`, `size` and `value` are valid for the duration of the call, and
        // `size` matches the byte size of the u32 out-buffer; no qualifier is passed.
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                &addr,
                0,
                std::ptr::null(),
                &mut size,
                &mut value as *mut u32 as *mut c_void,
            )
        };
        (status == 0).then_some(value)
    }

    pub fn mic_in_use() -> bool {
        let Some(dev) = get_u32(SYSTEM_OBJECT, DEFAULT_INPUT_DEVICE) else {
            return false;
        };
        if dev == 0 {
            return false; // kAudioObjectUnknown: no input device
        }
        get_u32(dev, IS_RUNNING_SOMEWHERE).map(|v| v != 0).unwrap_or(false)
    }

    pub fn running_meeting_apps() -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        // NSWorkspace.sharedWorkspace / runningApplications are safe to call from any thread.
        let apps = NSWorkspace::sharedWorkspace().runningApplications();
        for app in apps.iter() {
            if let Some(id) = app.bundleIdentifier() {
                if let Some(name) = meeting_app_name(&id.to_string()) {
                    if !out.iter().any(|n| n == name) {
                        out.push(name.to_string());
                    }
                }
            }
        }
        out
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn mic_in_use() -> bool {
        false
    }
    pub fn running_meeting_apps() -> Vec<String> {
        Vec::new()
    }
}
