//! Display windows (docs/14-display-windows.md): layouts, the layout picker's placing (P1),
//! new / close (W6, W7), and each window keeping its own settings and Hide / Isolate (§2).

use custom_features::selection::{Bits, Set};
use custom_features::windows::{self, Layout};
use custom_features::{visibility, vv};

#[test]
fn w1_layouts_cover_the_area_without_overlap() {
    for layout in Layout::ALL {
        let rects = layout.rects();
        assert_eq!(rects.len(), layout.zones(), "{layout:?}");
        let area: f32 = rects.iter().map(|[l, t, r, b]| (r - l) * (b - t)).sum();
        assert!((area - 1.0).abs() < 1e-5, "{layout:?} covers {area}");
        assert_eq!(Layout::from_key(layout.key()), Some(layout));
    }
    // L2: big left two thirds, small right third.
    assert_eq!(Layout::BigSmall.rects()[0][2], 2.0 / 3.0);
    assert_eq!(Layout::from_key("9"), None);
}

/// The windows are global: one test, so no other test changes them meanwhile.
#[test]
fn p1_w6_w7_r1_placing_closing_and_own_state_per_window() {
    windows::reset();
    visibility::show_all();
    // From one window to 1 + 2 with the window placed top right (zone 1): two copies are added.
    let first = windows::current().active;
    windows::set_layout(Layout::OneTwo, Some((first, 1)));
    let w = windows::current();
    assert_eq!(w.windows.len(), 3);
    assert_eq!(w.windows[1].id, first, "placed in the zone clicked (P1)");
    assert_eq!(w.active, first, "the active window stays active");
    assert!(w.windows.iter().filter(|x| x.id != first).all(|x| x.saved.is_some()), "copies of the active one (W7)");

    // Own settings and own Hide / Isolate per window (§2, R1).
    let mut s = vv::settings();
    s.legend = !s.legend;
    vv::set_quiet(s.clone());
    let mut hidden = Set::default();
    hidden.shells = Bits::with_len(3);
    hidden.shells.set(2, true);
    visibility::set_hidden(hidden.clone());
    let other = w.windows[0].id;
    windows::activate(other);
    assert_eq!(windows::current().active, other);
    assert_ne!(vv::settings().legend, s.legend, "the other window has its own settings");
    assert!(visibility::hidden().is_empty(), "and its own hidden set");
    windows::activate(first);
    assert_eq!(vv::settings().legend, s.legend, "back: the first window's settings again");
    assert_eq!(visibility::hidden(), hidden, "and its hidden set");

    // At most four; the last window cannot be closed (W6).
    windows::set_layout(Layout::Four, None);
    assert!(!windows::new_window(), "at most four");
    let ids: Vec<u64> = windows::current().windows.iter().map(|x| x.id).collect();
    for id in &ids[1..] {
        assert!(windows::close(*id));
    }
    assert_eq!(windows::current().layout, Layout::One);
    assert!(!windows::close(ids[0]), "the last window stays");

    // Closing the active window activates a neighbour; from 4 to 1 the active one is kept.
    assert!(windows::new_window());
    let w = windows::current();
    let (a, b) = (w.windows[0].id, w.windows[1].id);
    windows::activate(b);
    windows::set_layout(Layout::One, None);
    assert_eq!(windows::current().windows.iter().map(|x| x.id).collect::<Vec<_>>(), vec![b], "the active one is kept (P1)");
    let _ = a;
    windows::reset();
    visibility::show_all();
    vv::set_quiet(vv::Settings::default());
}
