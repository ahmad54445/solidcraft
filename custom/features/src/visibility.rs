//! Hide / Isolate (docs/13 §8): elements hidden by hand, one bit per element, on top of the
//! Visibility/Graphics categories (a category off hides all its elements whatever is set here).
//! A hidden element is not drawn, not snapped, not labelled and not selectable (rule R2): the
//! view marks it `Look::Hidden`, and every tool reads that.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, PoisonError, RwLock};

use crate::selection::{Bits, Set};
use crate::transfer::Geometry;

static HIDDEN: LazyLock<RwLock<Set>> = LazyLock::new(|| RwLock::new(Set::default()));
static VERSION: AtomicU64 = AtomicU64::new(1);

/// Changes whenever the hidden set does (the Browser's eyes follow it).
pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// The elements hidden by hand.
pub fn hidden() -> Set {
    HIDDEN.read().unwrap_or_else(PoisonError::into_inner).clone()
}

fn change(f: impl FnOnce(&mut Set)) {
    f(&mut HIDDEN.write().unwrap_or_else(PoisonError::into_inner));
    VERSION.fetch_add(1, Ordering::Relaxed);
}

/// Mark the hidden elements in the looks the view is built from.
pub fn apply(shells: &mut [crate::vv::Look], frames: &mut [crate::vv::Look]) {
    let h = HIDDEN.read().unwrap_or_else(PoisonError::into_inner);
    for e in h.shells.ones() {
        if let Some(l) = shells.get_mut(e) {
            *l = crate::vv::Look::Hidden;
        }
    }
    for e in h.frames.ones() {
        if let Some(l) = frames.get_mut(e) {
            *l = crate::vv::Look::Hidden;
        }
    }
}

/// Hide these elements (added to what is hidden).
pub fn hide(set: &Set) {
    change(|h| {
        for e in set.shells.ones() {
            h.shells.set(e, true);
        }
        for e in set.frames.ones() {
            h.frames.set(e, true);
        }
    });
}

/// Show these elements again.
pub fn show(set: &Set) {
    change(|h| {
        for e in set.shells.ones() {
            h.shells.set(e, false);
        }
        for e in set.frames.ones() {
            h.frames.set(e, false);
        }
    });
}

/// Isolate: everything except these elements is hidden; these are shown.
pub fn isolate(g: &Geometry, set: &Set) {
    change(|h| {
        let mut shells = Bits::with_len(g.shells.len());
        let mut frames = Bits::with_len(g.frames.len());
        for e in 0..g.shells.len() {
            shells.set(e, !set.shells.get(e));
        }
        for e in 0..g.frames.len() {
            frames.set(e, !set.frames.get(e));
        }
        *h = Set { shells, frames };
    });
}

/// Show everything hidden by hand.
pub fn show_all() {
    change(|h| *h = Set::default());
}

/// How many of `set` are hidden by hand, and how many it has.
pub fn hidden_of(set: &Set) -> (usize, usize) {
    let h = HIDDEN.read().unwrap_or_else(PoisonError::into_inner);
    let hidden = set.shells.ones().filter(|e| h.shells.get(*e)).count() + set.frames.ones().filter(|e| h.frames.get(*e)).count();
    let (s, f) = set.counts();
    (hidden, s + f)
}

/// An eye click on a row standing for `set`: hide it all when any of it is shown, else show it.
pub fn toggle(set: &Set) {
    let (hidden, total) = hidden_of(set);
    if hidden < total { hide(set) } else { show(set) }
}
