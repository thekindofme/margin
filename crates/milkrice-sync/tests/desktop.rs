//! The Mac side end to end, with a simulated iPhone: set up, pair (QR + matching code), import a
//! phone meeting, answer its request for notes, and hand the Mac's own meetings to the phone.
//! One test function, because it points `MILKRICE_HOME` at a temporary folder.
use milkrice_core::store::{Meeting, Store};
use milkrice_sync::{
    crypto,
    desktop::{self, Pairing},
    folder::{DeviceCard, Vault},
    merge::{self, Clock, MeetingState},
    pair,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn wall() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}

#[test]
fn mac_and_simulated_iphone() {
    let tmp = std::env::temp_dir().join(format!("milkrice-sync-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::env::set_var("MILKRICE_HOME", tmp.join("home"));
    std::env::set_var("MILKRICE_SYNC_KEYFILE", tmp.join("key.hex"));
    let folder = tmp.join("drive/Milkrice Sync");

    // A Mac meeting that already exists before sync is turned on.
    let store = Store::open(&milkrice_core::paths::db_path()).unwrap();
    let now = chrono::Utc::now();
    store
        .create_meeting(&Meeting { id: "mac-1".into(), title: "Design sync".into(), created_at: now, notes: "ship v2".into(), status: "done".into(), ..Default::default() })
        .unwrap();

    // 1. Set up.
    let cfg = desktop::init(Some(folder.clone())).unwrap();
    assert!(desktop::folder_has_vault(&folder));
    assert!(desktop::init(Some(folder.clone())).is_err(), "a second init must refuse");
    let mac_vault = desktop::open_vault(&cfg).unwrap();

    // 2. Pair: the Mac shows a QR; the phone answers; both show the same code.
    let mut p = Pairing::start(&mac_vault);
    assert!(desktop::qr_svg(&p.qr).unwrap().starts_with("<?xml") || desktop::qr_svg(&p.qr).unwrap().contains("<svg"));
    assert!(p.poll().unwrap().is_none());
    let invite = pair::Invite::parse(&p.qr).unwrap();
    let phone_secret: [u8; 32] = crypto::random_bytes();
    let phone_pub = pair::public_key(&phone_secret);
    let d = pair::derive(&phone_secret, &invite.mac_pub, &invite, &phone_pub).unwrap();
    let card = DeviceCard { name: "Test iPhone".into(), kind: "iphone".into(), last_seen: "2026-11-21T00:00:00Z".into() };
    let sealed = crypto::box_seal(&d.pair_key, &invite.session, &crypto::random_bytes(), &serde_json::to_vec(&card).unwrap()).unwrap();
    let req = json!({"v": 1, "pub": crypto::b64(&phone_pub), "card": crypto::b64(&sealed)});
    std::fs::create_dir_all(mac_vault.pair_dir()).unwrap();
    std::fs::write(mac_vault.pair_dir().join(format!("{}.req", invite.session_hex())), serde_json::to_vec(&req).unwrap()).unwrap();
    let joining = p.poll().unwrap().expect("the Mac sees the phone");
    assert_eq!(joining.name, "Test iPhone");
    assert_eq!(joining.code, d.code, "both screens show the same 6-digit code");
    assert_eq!(p.accept(&mac_vault).unwrap(), "Test iPhone");
    let resp: pair::Response =
        serde_json::from_slice(&std::fs::read(mac_vault.pair_dir().join(format!("{}.resp", invite.session_hex()))).unwrap()).unwrap();
    let (key_id, key) = pair::open_grant(&d, &invite, &resp).unwrap();
    assert_eq!(key, mac_vault.key, "the phone received the vault key");
    let phone_vault = Vault { root: folder.clone(), vault_id: invite.vault_id, key_id, key };
    phone_vault.write_device("phone", &card).unwrap();

    // 3. The phone records a meeting and asks the Mac for notes.
    let mut clock = Clock::new("phone");
    let mut pstate = MeetingState::new();
    let mut fields: BTreeMap<String, Value> = BTreeMap::new();
    fields.insert("title".into(), json!("Coffee with Sam"));
    fields.insert("notes".into(), json!(""));
    fields.insert("transcript".into(), json!([[0, 2000, "Speaker", "Shall we ship on Friday?"], [2000, 4000, "Me", "Yes, Friday."]]));
    fields.insert("created_at".into(), json!("2026-11-21T09:00:00Z"));
    fields.insert("duration_ms".into(), json!(4000));
    fields.insert("origin".into(), json!("iphone"));
    fields.insert("mode".into(), json!("room"));
    fields.insert("deleted".into(), json!(false));
    fields.insert("notes_request".into(), json!("2026-11-21T09:01:00Z"));
    let out = merge::merge("phone-1", &fields, &mut pstate, &[], &mut clock, wall());
    phone_vault.write_record(&out.record).unwrap();

    // 4. One Mac pass imports it, flags the request, and exports the Mac meeting.
    let r = desktop::sync_pass(&cfg, &mac_vault, &store).unwrap();
    assert_eq!(r.imported, vec!["phone-1".to_string()]);
    assert_eq!(r.requests, vec!["phone-1".to_string()]);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    let m = store.get_meeting("phone-1").unwrap().expect("phone meeting is on the Mac");
    assert_eq!(m.title, "Coffee with Sam");
    let segs = store.segments("phone-1").unwrap();
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[1].text, "Yes, Friday.");
    assert!(desktop::iphone_meetings().contains("phone-1"));
    assert!(!desktop::iphone_meetings().contains("mac-1"));

    // 5. The Mac writes notes (as the app does with the chosen AI); the next pass sends them back.
    store.set_field("phone-1", "enhanced", "- Ship on Friday").unwrap();
    store.set_field("phone-1", "enhanced_by", "claude:haiku").unwrap();
    let r = desktop::sync_pass(&cfg, &mac_vault, &store).unwrap();
    assert!(r.requests.is_empty(), "the request is answered once notes are newer");
    assert!(r.imported.is_empty());

    // 6. The phone reads the Mac's records: the notes for its meeting, and the Mac's own meeting.
    let (recs, problems) = phone_vault.read_records("phone").unwrap();
    assert!(problems.is_empty());
    let mac_rec = &recs["phone-1"][0];
    assert_eq!(mac_rec.fields["enhanced"].v, json!("- Ship on Friday"));
    let out = merge::merge("phone-1", &fields, &mut pstate, &recs["phone-1"], &mut clock, wall());
    assert_eq!(out.apply.get("enhanced"), Some(&json!("- Ship on Friday")));
    assert_eq!(recs["mac-1"][0].fields["title"].v, json!("Design sync"));
    assert_eq!(recs["mac-1"][0].fields["origin"].v, json!("mac"));

    // 6b. A phone meeting whose records never said where it was recorded (only its mode) is
    // still the phone's, and the Mac never claims it as its own.
    let mut f2 = fields.clone();
    f2.remove("origin");
    f2.remove("notes_request");
    let mut s2 = MeetingState::new();
    let out2 = merge::merge("phone-2", &f2, &mut s2, &[], &mut clock, wall());
    phone_vault.write_record(&out2.record).unwrap();
    desktop::sync_pass(&cfg, &mac_vault, &store).unwrap();
    desktop::sync_pass(&cfg, &mac_vault, &store).unwrap();
    assert!(desktop::iphone_meetings().contains("phone-2"));
    let (recs2, _) = phone_vault.read_records("phone").unwrap();
    assert!(recs2["phone-2"][0].fields.get("origin").is_none(), "the Mac must not claim a phone meeting");
    assert_eq!(recs2["phone-2"][0].fields["mode"].v, json!("room"));
    assert_eq!(recs2["mac-1"][0].fields["origin"].v, json!("mac"), "the Mac keeps sending its own origin");

    // 7. Status lists both devices; a quiet pass rewrites nothing (no needless uploads).
    let st = desktop::status(&cfg, &mac_vault).unwrap();
    assert_eq!(st.devices.len(), 2);
    assert!(st.devices.iter().any(|d| d.this_mac));
    assert!(st.devices.iter().any(|d| d.kind == "iphone" && d.name == "Test iPhone"));
    let r = desktop::sync_pass(&cfg, &mac_vault, &store).unwrap();
    assert_eq!(r.exported, 0);

    // 8. A deletion on the phone reaches the Mac.
    fields.insert("deleted".into(), json!(true));
    let out = merge::merge("phone-1", &fields, &mut pstate, &recs["phone-1"], &mut clock, wall());
    phone_vault.write_record(&out.record).unwrap();
    let r = desktop::sync_pass(&cfg, &mac_vault, &store).unwrap();
    assert_eq!(r.imported, vec!["phone-1".to_string()]);
    assert!(store.get_meeting("phone-1").unwrap().is_none());

    // 9. Leaving forgets the setup but keeps the meetings.
    desktop::leave().unwrap();
    assert!(desktop::config().is_err());
    assert!(store.get_meeting("mac-1").unwrap().is_some());

    let _ = std::fs::remove_dir_all(&tmp);
}
