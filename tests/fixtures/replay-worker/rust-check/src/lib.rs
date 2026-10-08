//! Fixed test-only browser corpus: genuine existing wire/state/cost interfaces.
//! No general admission policy, native pixels, timing, RSS, or playback authority.
#![cfg(test)]

use board_media::replay_cost::{CostOptions, CostScope, estimate};
use board_media::replay_state::check_core_v1;
use board_media::replay_wire::decode;
use sha2::{Digest, Sha256};

include!("../../generated/core-cases.rs");

#[test]
fn exact_browser_probe_corpus_passes_existing_state_and_cost_interfaces() {
    assert_eq!(PROBE_CASES.len(), 18);
    for &(name, digest, bytes) in PROBE_CASES {
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), digest, "{name}");
        let candidate = decode(bytes).expect(name);
        assert!(candidate.metadata.width <= 24 && candidate.metadata.height <= 24, "{name}");
        assert!(candidate.events.len() <= 96, "{name}");
        let checked = check_core_v1(&candidate).expect(name);
        let report = estimate(&checked, CostOptions { include_final_flatten: true }).expect(name);
        assert_eq!(report.counters(CostScope::Total).events, candidate.events.len() as u64, "{name}");
        assert!(report.counters(CostScope::FinalFlatten).flatten_draw_calls <= 9, "{name}");
        // The native differential presents after every event. Those REPEATED
        // flattens and transfers are explicitly outside this one-time estimate.
    }
}
