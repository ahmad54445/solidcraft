//! The CSiBridge dialog's logic, with a fixture source (no CSiBridge needed).

use custom_features::csi::{CaseStatus, Dialog, Instance, Phase, Snapshot, Source, parse_snapshot};

struct Fixture(Result<Snapshot, String>);

impl Source for Fixture {
    fn title(&self) -> String {
        "fixture".into()
    }
    fn instances(&self) -> Result<Vec<Instance>, String> {
        Ok(vec![Instance { pid: 7, title: "fixture".into() }])
    }
    fn read(&self, _pid: u32) -> Result<Snapshot, String> {
        self.0.clone()
    }
}

fn snapshot(shells: usize) -> Snapshot {
    Snapshot {
        program: "CSiBridge 25.3.1".into(),
        model: "m.bdb".into(),
        shells,
        frames: 2,
        cases: vec![("A".into(), "LinearStatic".into(), CaseStatus::Finished), ("B".into(), "LinearStatic".into(), CaseStatus::NotRun)],
        combos: vec![("C1".into(), "Linear Additive".into(), vec![("A".into(), 1.35)])],
    }
}

#[test]
fn select_all_skips_cases_that_were_not_run() {
    let mut d = Dialog::from_source(&Fixture(Ok(snapshot(3))), 7);
    assert_eq!(d.phase, Phase::Ready);
    assert_eq!(d.cases[1].blocked.as_deref(), Some("not run"));
    Dialog::set_all(&mut d.cases, true);
    assert_eq!(d.ticked_cases(), 1, "B is not run, so it stays unticked");
    Dialog::set_all(&mut d.cases, false);
    assert_eq!(d.ticked_cases(), 0);
}

#[test]
fn send_needs_a_selection_and_a_tick() {
    let mut d = Dialog::from_source(&Fixture(Ok(snapshot(3))), 7);
    assert!(!d.can_send(), "nothing ticked");
    d.combos[0].checked = true;
    assert!(d.can_send());
    let mut empty = Dialog::from_source(&Fixture(Ok(Snapshot { frames: 0, ..snapshot(0) })), 7);
    empty.combos[0].checked = true;
    assert!(!empty.can_send(), "nothing selected in CSiBridge");
}

#[test]
fn send_stores_the_ticked_items() {
    let mut d = Dialog::from_source(&Fixture(Ok(snapshot(3))), 7);
    d.cases[0].checked = true;
    d.combos[0].checked = true;
    assert!(d.send());
    custom_features::model::read(|f| {
        assert_eq!(f.model.load_cases.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["A"]);
        assert_eq!(f.model.load_combinations[0].factors, [("A".to_string(), 1.35)]);
        assert_eq!((f.status.selected_shells, f.status.selected_frames), (3, 2));
    });
}

#[test]
fn a_source_error_is_shown() {
    let d = Dialog::from_source(&Fixture(Err("not running".into())), 7);
    assert_eq!(d.phase, Phase::Error("not running".into()));
    assert!(!d.can_send());
}

#[test]
fn api_status_numbers() {
    assert_eq!(CaseStatus::from_api(4), CaseStatus::Finished);
    assert_eq!(CaseStatus::from_api(1), CaseStatus::NotRun);
}

#[test]
fn agent_snapshot_is_parsed() {
    // The shape csi-agent sends (docs/09-agent-protocol.md), as read from CSiBridge 25.3.1.
    let r = serde_json::json!({"v":1,"id":1,"ok":true,"program":"CSiBridge 25.3.1","model":"C:\\m.bdb",
        "shells":2772,"frames":293,
        "cases":[{"name":"DC","type":"LinearStatic","status":4},{"name":"MODAL","type":"Modal","status":1}],
        "combos":[{"name":"TU","type":"Envelope","items":[{"name":"TU-","kind":"case","factor":1.0},{"name":"TU+","kind":"case","factor":1.0}]}]});
    let s = parse_snapshot(&r).unwrap();
    assert_eq!((s.shells, s.frames), (2772, 293));
    assert_eq!(s.cases[1].2, CaseStatus::NotRun);
    assert_eq!(s.combos[0].2, [("TU-".to_string(), 1.0), ("TU+".to_string(), 1.0)]);
    let d = Dialog::from_source(&Fixture(Ok(s)), 1);
    assert_eq!(d.cases[1].blocked.as_deref(), Some("not run"));
}
