//! WordPress's shortcode functions, recorded from WordPress 7.1.5 (`compat/shortcode`), replayed exactly through the
//! native ones, with every combination of scanner and attribute parser.
//!
//! PHP boots once per process, so this file has its own `main()` (`harness = false`).

use std::cell::Cell;
use std::process::ExitCode;
use std::rc::Rc;

use speedyspec_compat::{replay, wordpress_release};
use speedyspec_container::Container;
use speedyspec_hooks_memory::MemoryHooks;
use speedyspec_php::{BootOptions, NativeArea, Php};
use speedyspec_wp_hooks_memory::MemoryHookStore;
use speedyspec_wp_hooks_php::WpHooksArea;
use speedyspec_wp_kses_php::{KsesArea, prepare_release};
use speedyspec_wp_shortcode::{FancyRegexScanner, HandAttsParser, HandScanner, RegexAttsParser};
use speedyspec_wp_shortcode_contracts::{AttsParse, AttsParser, ShortcodeMatches, ShortcodeScanner, TagSet};
use speedyspec_wp_shortcode_php::{ShortcodeArea, shortcode_scenario_runner};

/// Strategies switched between replays, since the area is fixed once PHP boots.
struct Switch {
    hand_scanner: Cell<bool>,
    hand_atts: Cell<bool>,
    hand: HandScanner,
    fancy: FancyRegexScanner,
    hand_parser: HandAttsParser,
    regex_parser: RegexAttsParser,
}

impl ShortcodeScanner for Switch {
    fn scan<'h>(&self, haystack: &'h [u8], tags: &TagSet) -> ShortcodeMatches<'h> {
        if self.hand_scanner.get() { self.hand.scan(haystack, tags) } else { self.fancy.scan(haystack, tags) }
    }
}

impl AttsParser for Switch {
    fn parse(&self, text: &[u8]) -> AttsParse {
        if self.hand_atts.get() { self.hand_parser.parse(text) } else { self.regex_parser.parse(text) }
    }
}

fn main() -> ExitCode {
    let release = match wordpress_release("7.1.5") {
        Ok(root) => root,
        Err(why) => {
            println!("FAILED: the WordPress release: {why}");
            return ExitCode::FAILURE;
        }
    };
    let switch = Rc::new(Switch {
        hand_scanner: Cell::new(true),
        hand_atts: Cell::new(true),
        hand: HandScanner::new(),
        fancy: FancyRegexScanner::new(),
        hand_parser: HandAttsParser::new(),
        regex_parser: RegexAttsParser::new(),
    });
    // The native kses functions replace the release's, so its kses.php runs only for the globals.
    let prepared = match prepare_release(&release, &release.with_file_name("wordpress-shortcode-port")) {
        Ok(root) => root,
        Err(why) => {
            println!("FAILED: preparing the release for kses: {why}");
            return ExitCode::FAILURE;
        }
    };
    let container = Rc::new(Container::new());
    let hooks = WpHooksArea::new(container.clone(), Rc::new(MemoryHookStore::new()));
    let kses = KsesArea::new(container.clone(), hooks.api());
    let shortcodes = ShortcodeArea::with_strategies(container, hooks.api(), kses.api(), switch.clone(), switch.clone());
    let areas: Vec<Rc<dyn NativeArea>> = vec![Rc::new(hooks), Rc::new(kses), Rc::new(shortcodes)];
    let php = match Php::boot(BootOptions { hooks: Rc::new(MemoryHooks::new()), preload: &[], areas }) {
        Ok(php) => php,
        Err(err) => {
            println!("FAILED: boot: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut runner = shortcode_scenario_runner(php, &prepared);
    let mut failed = 0;
    for (hand_scanner, hand_atts) in [(true, true), (true, false), (false, true), (false, false)] {
        switch.hand_scanner.set(hand_scanner);
        switch.hand_atts.set(hand_atts);
        let label = format!(
            "{} scanner, {} attributes",
            if hand_scanner { "hand" } else { "fancy-regex" },
            if hand_atts { "hand" } else { "regex" }
        );
        let mismatches = replay("shortcode", &mut runner);
        for mismatch in &mismatches {
            let text = mismatch.to_string();
            println!("  mismatch ({label}): {}", &text[..text.floor_char_boundary(900)]);
            if let (Some(e), Some(a)) = (&mismatch.expected, &mismatch.actual) {
                let (e, a) = (format!("{e:?}"), format!("{a:?}"));
                println!("    expected {}", &e[..e.floor_char_boundary(600)]);
                println!("    actual   {}", &a[..a.floor_char_boundary(600)]);
            }
        }
        if mismatches.is_empty() {
            println!("ok: replayed_scenarios_match_wordpress ({label})");
        } else {
            failed += 1;
            println!("FAILED: replayed_scenarios_match_wordpress ({label}): {} mismatches", mismatches.len());
        }
    }
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
