//! Named views with view templates (docs/15-named-views.md): every window shows a named view, a
//! view has one template, the template rules the parts its switches name and is linked (N6).
//! The windows are global: one test at a time.

use custom_features::windows::{self, Include, Layout};
use custom_features::{results_view, visibility, vv};

static GLOBAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn start() -> std::sync::MutexGuard<'static, ()> {
    let g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    windows::reset();
    visibility::show_all();
    vv::set_quiet(vv::Settings::default());
    for t in windows::templates() {
        windows::delete_template(&t.name);
    }
    g
}

fn view_of_active() -> windows::NamedView {
    let w = windows::current();
    w.view_of(w.active).cloned().expect("the active window shows a view")
}

#[test]
fn n1_every_window_shows_a_named_view() {
    let _g = start();
    assert_eq!(view_of_active().name, "View 1");
    windows::set_layout(Layout::Two, None);
    let w = windows::current();
    let names: Vec<String> = w.windows.iter().map(|x| x.view.clone()).collect();
    assert_eq!(names, vec!["View 1", "View 2"], "a new window makes a new view");
    // Closing a window keeps its view (R6); a view shown in a window cannot be deleted.
    windows::set_layout(Layout::One, None);
    assert_eq!(windows::views().len(), 2);
    assert!(!windows::delete_view("View 1"), "shown in a window");
    assert!(windows::delete_view("View 2"));
    // Rename: names are unique (R7).
    assert!(windows::duplicate_view("View 1").is_some());
    assert!(!windows::rename_view("View 1", "View 1 copy"), "taken");
    assert!(windows::rename_view("View 1", "Deck plan"));
    assert_eq!(view_of_active().name, "Deck plan", "the window follows the new name");
}

#[test]
fn n3_n6_r1_r3_a_template_rules_its_parts_and_is_linked() {
    let _g = start();
    let mut s = vv::Settings::default();
    s.values = true;
    vv::set_quiet(s);
    assert!(windows::save_template("Values", windows::current().active));
    let t = windows::templates().into_iter().find(|t| t.name == "Values").expect("saved");
    assert_eq!(t.include, Include::new_template(), "V/G and result; camera and Hide / Isolate stay the view's (N5)");

    // Two windows; the second view gets the template, the first has none.
    windows::set_layout(Layout::Two, None);
    vv::set_quiet(vv::Settings::default());
    let w = windows::current();
    let (first, second) = (w.windows[0].id, w.windows[1].id);
    let second_view = w.windows[1].view.clone();
    assert!(windows::set_view_template(&second_view, Some("Values")));
    windows::activate(second);
    assert!(vv::settings().values, "the template's Visibility/Graphics (N4)");

    // Linked (N6): changed in a view that uses it, the template changes – and so do its other views.
    windows::activate(first);
    let first_view = view_of_active().name;
    assert!(windows::set_view_template(&first_view, Some("Values")));
    let mut changed = vv::settings();
    changed.legend = !changed.legend;
    vv::set_quiet(changed.clone());
    std::thread::sleep(std::time::Duration::from_millis(300));
    windows::sync();
    let t = windows::templates().into_iter().find(|t| t.name == "Values").expect("still there");
    assert_eq!(t.state.settings.legend, changed.legend, "the template took the change");
    windows::activate(second);
    assert_eq!(vv::settings().legend, changed.legend, "the other view follows");

    // A part switched off stays the view's own (R1).
    assert!(windows::set_include("Values", Include { vg: false, ..Include::new_template() }));
    vv::set_quiet(vv::Settings::default());
    std::thread::sleep(std::time::Duration::from_millis(300));
    windows::sync();
    let t = windows::templates().into_iter().find(|t| t.name == "Values").expect("still there");
    assert!(t.state.settings.values, "V/G not ruled: the template keeps its own");

    // Detach (R3): the view keeps what it shows.
    assert!(windows::set_include("Values", Include::new_template()));
    assert!(vv::settings().values, "ruled again: the template's settings");
    assert!(windows::set_view_template(&view_of_active().name, None));
    assert!(vv::settings().values, "detached: nothing jumps");
    assert_eq!(view_of_active().template, None);

    // Deleting a template detaches its views (R7).
    assert!(windows::delete_template("Values"));
    assert!(windows::current().views.iter().all(|v| v.template.is_none()));
}

#[test]
fn r6_opening_a_view() {
    let _g = start();
    windows::set_layout(Layout::Two, None);
    let w = windows::current();
    let (first, second) = (w.windows[0].id, w.windows[1].id);
    let second_view = w.windows[1].view.clone();
    // A view shown in another window: that window becomes active.
    assert!(windows::open_view(&second_view, first));
    assert_eq!(windows::current().active, second);
    // A view not shown: it opens in the window, whose old view keeps what it showed.
    windows::activate(first);
    let mut s = vv::Settings::default();
    s.legend = false;
    vv::set_quiet(s);
    let spare = windows::duplicate_view("View 1").expect("copy");
    let mut other = vv::Settings::default();
    other.legend = true;
    vv::set_quiet(other);
    assert!(windows::open_view(&spare, first));
    assert!(!vv::settings().legend, "the copy shows what View 1 showed when copied");
    assert!(windows::open_view("View 1", first));
    assert!(vv::settings().legend, "View 1 kept what it showed when it was left");
    assert!(!windows::open_view("nothing", first));
}

#[test]
fn s3_starting_templates_and_the_saved_views() {
    let _g = start();
    let start_set = windows::starting_templates();
    let names: Vec<&str> = start_set.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["Plate forces", "Plate membrane", "Beam forces", "Beam axial", "Geometry"]);
    let comp = |n: &str| start_set.iter().find(|t| t.name == n).map(|t| (t.state.choice.shell, t.state.choice.frame)).expect("there");
    assert_eq!(results_view::SHELL_COMPONENTS[comp("Plate forces").0], "M11");
    assert_eq!(results_view::SHELL_COMPONENTS[comp("Plate membrane").0], "F11");
    assert_eq!(results_view::FRAME_COMPONENTS[comp("Beam forces").1], "M3");
    assert_eq!(results_view::FRAME_COMPONENTS[comp("Beam axial").1], "P");

    // Views, their templates and the links are kept (§4).
    assert!(windows::save_template("Plan", windows::current().active));
    assert!(windows::set_view_template("View 1", Some("Plan")));
    windows::set_layout(Layout::Two, None);
    let stored = windows::stored();
    let text = serde_json::to_string(&stored).expect("JSON");
    let back: windows::Stored = serde_json::from_str(&text).expect("read back");
    windows::reset();
    windows::restore(&back);
    let w = windows::current();
    assert_eq!(w.views.len(), 2);
    assert_eq!(w.views.iter().find(|v| v.name == "View 1").and_then(|v| v.template.clone()), Some("Plan".into()));
    assert_eq!(w.windows.iter().map(|x| x.view.clone()).collect::<Vec<_>>(), vec!["View 1", "View 2"]);
    // A file of §14 (no views): one view per window.
    let old: windows::Stored = serde_json::from_str(r#"{"layout":"2","windows":[{},{}]}"#).expect("old file");
    windows::restore(&old);
    assert_eq!(windows::current().views.len(), 2);
    windows::delete_template("Plan");
}
