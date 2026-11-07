//! Unit factors to SI: a wrong factor here would scale every displayed result.

use custom_features::units::{Force, Length, Moment, Stress, Units};

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * a.abs().max(b.abs())
}

#[test]
fn factors_to_si() {
    assert_eq!(Length::Mm.to_si(), 1e-3);
    assert_eq!(Length::Cm.to_si(), 1e-2);
    assert_eq!(Force::KN.to_si(), 1e3);
    assert!(close(Moment::KNM.to_si(), Force::KN.to_si() * Length::M.to_si()));
    assert!(close(Moment::NMm.to_si(), Force::N.to_si() * Length::Mm.to_si()));
    // N/mm² = MPa = 1e6 Pa
    assert!(close(Stress::NMm2.to_si(), Force::N.to_si() / (Length::Mm.to_si() * Length::Mm.to_si())));
    assert_eq!(Stress::KNM2.to_si(), 1e3);
}

#[test]
fn default_is_kn_m() {
    let u = Units::default();
    assert_eq!(u.short(), "kN, m");
    assert_eq!(u.moment.symbol(), "kN·m");
    assert_eq!(u.stress.symbol(), "kN/m²");
}
