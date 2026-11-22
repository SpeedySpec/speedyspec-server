//! Any bytes through the MIDI parser. Any result is fine; a panic or hang is a finding.
#![no_main]

use libfuzzer_sys::fuzz_target;
use speedyspec_wp_media_contracts::ProbeLimits;

fuzz_target!(|data: &[u8]| {
    let _ = speedyspec_wp_media_midi::parse(data, &ProbeLimits::default());
});
