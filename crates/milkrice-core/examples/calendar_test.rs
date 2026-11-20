use chrono::{Duration, Utc};
use milkrice_core::calendar::{self, CalendarAuth};
use milkrice_core::detect;

fn main() {
    let _ = env_logger::builder().filter_level(log::LevelFilter::Info).try_init();
    let mut auth = calendar::auth_status();
    println!("auth: {auth:?}");
    if auth == CalendarAuth::NotDetermined {
        println!("requesting access (waits up to ~120s)...");
        match calendar::request_access() {
            Ok(g) => println!("granted: {g}"),
            Err(e) => println!("request error: {e}"),
        }
        auth = calendar::auth_status();
        println!("auth now: {auth:?}");
    }
    if auth == CalendarAuth::Authorized {
        let now = Utc::now();
        match calendar::events_between(now, now + Duration::days(7)) {
            Ok(evs) => {
                println!("{} events in next 7 days", evs.len());
                for e in evs {
                    println!("- {} [{} -> {}] cal={} color={:?} all_day={}", e.title, e.start, e.end, e.calendar, e.calendar_color, e.all_day);
                    println!("    location={:?} meeting_url={:?}", e.location, e.meeting_url);
                    for a in e.attendees {
                        println!("    attendee: {} <{}> self={}", a.name, a.email.unwrap_or_default(), a.is_self);
                    }
                }
            }
            Err(e) => println!("events error: {e}"),
        }
    }
    println!("mic_in_use: {}", detect::mic_in_use());
    println!("running_meeting_apps: {:?}", detect::running_meeting_apps());
    println!("meeting_app_using_mic: {:?}", detect::meeting_app_using_mic());
}
