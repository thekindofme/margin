//! Meeting detection without calendar: is another app using the mic, and which meeting apps run.
//! OWNER: calendar worker.

/// True when any process is using the default input device (CoreAudio
/// kAudioDevicePropertyDeviceIsRunningSomewhere). Includes us when we are recording.
pub fn mic_in_use() -> bool {
    imp::mic_in_use()
}

/// True when any process is playing through the default output device. While nothing plays,
/// ScreenCaptureKit may deliver no audio buffers at all, so a gap then isn't a lost stream.
pub fn output_in_use() -> bool {
    imp::output_in_use()
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

/// The known meeting app that is itself capturing audio input right now, using CoreAudio's
/// per-process attribution (macOS 14.2+). Unlike `meeting_app_using_mic`, Margin's own
/// recording doesn't count. `None` when per-process attribution isn't available.
pub fn meeting_app_capturing_input() -> Option<Option<String>> {
    imp::meeting_app_capturing_input()
}

/// Meeting app name for a process bundle id, including helper processes ("<app id>.<helper>").
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn meeting_app_for_process(bundle_id: &str) -> Option<&'static str> {
    meeting_app_name(bundle_id).or_else(|| {
        let (parent, _) = bundle_id.rsplit_once('.')?;
        meeting_app_for_process(parent)
    })
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
    use super::{meeting_app_for_process, meeting_app_name};
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
        fn AudioObjectGetPropertyDataSize(
            object: u32,
            address: *const AudioObjectPropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            data_size: *mut u32,
        ) -> i32;
    }

    const SYSTEM_OBJECT: u32 = 1;
    const DEFAULT_INPUT_DEVICE: u32 = 0x6449_6E20; // 'dIn '
    const DEFAULT_OUTPUT_DEVICE: u32 = 0x644F_7574; // 'dOut'
    const IS_RUNNING_SOMEWHERE: u32 = 0x676F_6E65; // 'gone'
    const SCOPE_GLOBAL: u32 = 0x676C_6F62; // 'glob'
    const ELEMENT_MAIN: u32 = 0;
    const PROCESS_OBJECT_LIST: u32 = 0x7072_7323; // 'prs#'
    const PROCESS_BUNDLE_ID: u32 = 0x7062_6964; // 'pbid'
    const PROCESS_IS_RUNNING_INPUT: u32 = 0x7069_7269; // 'piri'

    /// Reads a list of AudioObjectIDs; None on any CoreAudio error.
    fn get_ids(object: u32, selector: u32) -> Option<Vec<u32>> {
        let addr = AudioObjectPropertyAddress { selector, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN };
        let mut size: u32 = 0;
        // SAFETY: valid address struct and out-pointer for the size query; no qualifier.
        let status = unsafe { AudioObjectGetPropertyDataSize(object, &addr, 0, std::ptr::null(), &mut size) };
        if status != 0 {
            return None;
        }
        let mut ids = vec![0u32; size as usize / std::mem::size_of::<u32>()];
        let mut size = (ids.len() * std::mem::size_of::<u32>()) as u32;
        // SAFETY: `ids` has room for `size` bytes, which CoreAudio won't exceed.
        let status = unsafe {
            AudioObjectGetPropertyData(object, &addr, 0, std::ptr::null(), &mut size, ids.as_mut_ptr() as *mut c_void)
        };
        if status != 0 {
            return None;
        }
        ids.truncate(size as usize / std::mem::size_of::<u32>());
        Some(ids)
    }

    /// Bundle id of a CoreAudio process object.
    fn bundle_id(process: u32) -> Option<String> {
        let addr = AudioObjectPropertyAddress { selector: PROCESS_BUNDLE_ID, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN };
        let mut cf: *mut c_void = std::ptr::null_mut();
        let mut size = std::mem::size_of::<*mut c_void>() as u32;
        // SAFETY: the property is a CFStringRef (pointer-sized out-buffer), returned +1 retained.
        let status = unsafe {
            AudioObjectGetPropertyData(process, &addr, 0, std::ptr::null(), &mut size, &mut cf as *mut *mut c_void as *mut c_void)
        };
        if status != 0 || cf.is_null() {
            return None;
        }
        // SAFETY: CFString is toll-free bridged to NSString and we own the +1 reference, which
        // `Retained` releases on drop.
        let s = unsafe { objc2::rc::Retained::from_raw(cf as *mut objc2_foundation::NSString) }?;
        Some(s.to_string())
    }

    pub fn meeting_app_capturing_input() -> Option<Option<String>> {
        // Fails before macOS 14.2, where the process object list doesn't exist.
        let processes = get_ids(SYSTEM_OBJECT, PROCESS_OBJECT_LIST)?;
        for p in processes {
            if get_u32(p, PROCESS_IS_RUNNING_INPUT).unwrap_or(0) == 0 {
                continue;
            }
            if let Some(name) = bundle_id(p).as_deref().and_then(meeting_app_for_process) {
                return Some(Some(name.to_string()));
            }
        }
        Some(None)
    }

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

    pub fn output_in_use() -> bool {
        match get_u32(SYSTEM_OBJECT, DEFAULT_OUTPUT_DEVICE) {
            Some(dev) if dev != 0 => get_u32(dev, IS_RUNNING_SOMEWHERE).map(|v| v != 0).unwrap_or(false),
            _ => false,
        }
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
    pub fn meeting_app_capturing_input() -> Option<Option<String>> {
        None
    }
    pub fn output_in_use() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn helper_processes_map_to_their_app() {
        assert_eq!(super::meeting_app_for_process("us.zoom.xos"), Some("Zoom"));
        assert_eq!(super::meeting_app_for_process("com.microsoft.teams2.helper"), Some("Microsoft Teams"));
        assert_eq!(super::meeting_app_for_process("com.apple.Safari"), None);
        assert_eq!(super::meeting_app_for_process("app.margin"), None);
    }
}
