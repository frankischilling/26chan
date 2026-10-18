use super::*;
use ProviderValue::{Available, Unavailable};

fn snapshot(digest: Option<Digest>, a: Counts, b: Counts) -> Snapshot {
    Snapshot {
        digest,
        template_226: a,
        template_6: b,
    }
}

#[test]
fn absence_and_bypass_do_not_need_a_history_provider() {
    assert_eq!(evaluate("g", true, Upload::Absent, Unavailable), Ok(false));
    for upload in [
        Upload::Absent,
        Upload::PresentUnknown,
        Upload::PresentKnown([1; 16]),
    ] {
        assert_eq!(evaluate("b", true, upload, Unavailable), Ok(false));
    }
    assert_eq!(
        evaluate("g", false, Upload::PresentUnknown, Unavailable),
        Err(AdmissionError::Unavailable)
    );
    assert_eq!(
        evaluate("g", false, Upload::PresentKnown([1; 16]), Unavailable),
        Err(AdmissionError::Unavailable)
    );
    assert_eq!(
        evaluate("BAD", false, Upload::Absent, Unavailable),
        Err(AdmissionError::InvalidPolicy)
    );
}

#[test]
fn complete_known_history_keeps_the_qualified_source_threshold() {
    use crate::content_admission::files::{PriorBanCounts, prior_ban_rejection};
    for a in 0..=THRESHOLD {
        for b in 0..=THRESHOLD {
            for worksafe in [false, true] {
                let observed = snapshot(
                    Some([1; 16]),
                    Counts {
                        matching: a,
                        known: a,
                        unknown: 0,
                    },
                    Counts {
                        matching: b,
                        known: b,
                        unknown: 0,
                    },
                );
                assert_eq!(
                    evaluate(
                        "g",
                        worksafe,
                        Upload::PresentKnown([1; 16]),
                        Available(observed)
                    ),
                    prior_ban_rejection(
                        "g",
                        worksafe,
                        Available(Some([1; 16])),
                        Available(PriorBanCounts {
                            digest: [1; 16],
                            template_226: u64::from(a),
                            template_6: u64::from(b),
                        })
                    )
                );
            }
        }
    }
}

#[test]
fn every_bounded_history_returns_only_a_provable_decision() {
    // Enumerate all admissible capped counters. For a known upload, unknown
    // events may contribute anywhere from zero to their upper bound. For an
    // unknown upload, every retained file could be nonmatching or matching.
    for ak in 0..=THRESHOLD {
        for am in 0..=ak {
            for au in 0..=THRESHOLD {
                for bk in 0..=THRESHOLD {
                    for bm in 0..=bk {
                        for bu in 0..=THRESHOLD {
                            for worksafe in [false, true] {
                                let a = Counts {
                                    matching: am,
                                    known: ak,
                                    unknown: au,
                                };
                                let b = Counts {
                                    matching: bm,
                                    known: bk,
                                    unknown: bu,
                                };
                                let lower = am + if worksafe { bm } else { 0 };
                                let unknown = au + if worksafe { bu } else { 0 };
                                let possible: Vec<_> =
                                    (0..=unknown).map(|extra| lower + extra >= 3).collect();
                                let expected = if possible.iter().all(|blocked| *blocked) {
                                    Ok(true)
                                } else if possible.iter().all(|blocked| !*blocked) {
                                    Ok(false)
                                } else {
                                    Err(AdmissionError::Unavailable)
                                };
                                assert_eq!(
                                    evaluate(
                                        "g",
                                        worksafe,
                                        Upload::PresentKnown([1; 16]),
                                        Available(snapshot(Some([1; 16]), a, b))
                                    ),
                                    expected
                                );
                                if am == 0 && bm == 0 {
                                    let total = ak + au + if worksafe { bk + bu } else { 0 };
                                    let expected = if total < 3 {
                                        Ok(false)
                                    } else {
                                        Err(AdmissionError::Unavailable)
                                    };
                                    assert_eq!(
                                        evaluate(
                                            "g",
                                            worksafe,
                                            Upload::PresentUnknown,
                                            Available(snapshot(None, a, b))
                                        ),
                                        expected
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn evidence_must_match_the_upload_and_its_count_bounds() {
    let empty = Counts::default();
    for upload in [Upload::PresentUnknown, Upload::PresentKnown([2; 16])] {
        assert_eq!(
            evaluate(
                "g",
                true,
                upload,
                Available(snapshot(Some([1; 16]), empty, empty))
            ),
            Err(AdmissionError::InvalidPolicy)
        );
    }
    for invalid in [
        Counts {
            matching: 1,
            known: 0,
            unknown: 0,
        },
        Counts {
            matching: 0,
            known: 4,
            unknown: 0,
        },
        Counts {
            matching: 0,
            known: 0,
            unknown: 4,
        },
    ] {
        assert_eq!(
            evaluate(
                "g",
                true,
                Upload::PresentKnown([1; 16]),
                Available(snapshot(Some([1; 16]), invalid, empty))
            ),
            Err(AdmissionError::InvalidPolicy)
        );
    }
    assert_eq!(
        evaluate(
            "g",
            true,
            Upload::PresentUnknown,
            Available(snapshot(
                None,
                Counts {
                    matching: 1,
                    known: 1,
                    unknown: 0
                },
                empty
            ))
        ),
        Err(AdmissionError::InvalidPolicy)
    );
}
