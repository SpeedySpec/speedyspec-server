use indexmap::IndexMap;
use speedyspec_wp_kses_contracts::{AllowedAttrs, AllowedHtml, AttrRules, Protocols};
use speedyspec_wp_kses_security::{Finding, Level, inspect};

fn allowed() -> AllowedHtml {
    let mut tags = IndexMap::new();
    for tag in ["p", "a", "img", "div", "span", "svg", "math"] {
        let mut attrs = IndexMap::new();
        for a in ["href", "src", "title", "class", "style", "data"] {
            attrs.insert(a.as_bytes().into(), AttrRules::Allowed);
        }
        tags.insert(tag.as_bytes().into(), AllowedAttrs::Attrs(attrs));
    }
    AllowedHtml { tags }
}

fn protocols() -> Protocols {
    Protocols(
        ["http", "https", "mailto"]
            .iter()
            .map(|p| p.as_bytes().into())
            .collect(),
    )
}

fn exploits(output: &[u8]) -> Vec<Finding> {
    inspect(output, &allowed(), &protocols(), false)
        .into_iter()
        .filter(|f| f.level == Level::Exploit)
        .collect()
}

fn has(output: &[u8], rule: &str) -> bool {
    exploits(output).iter().any(|f| f.rule == rule)
}

#[test]
fn script_is_an_exploit() {
    assert!(has(b"<p>x</p><script>alert(1)</script>", "element"));
}

#[test]
fn event_handlers_are_exploits() {
    assert!(has(b"<img src=x onerror=alert(1)>", "event-handler"));
    assert!(has(b"<p ONCLICK=\"x\">y</p>", "event-handler"));
}

#[test]
fn entity_hidden_javascript_is_an_exploit() {
    assert!(has(
        b"<a href=\"&#106;ava&#x09;script:alert(1)\">x</a>",
        "script-url"
    ));
    assert!(has(b"<a href=\" \x01javascript:x\">x</a>", "script-url"));
    assert!(has(
        b"<img src=\"data:text/html,<script>alert(1)</script>\">",
        "script-url"
    ));
    assert!(!has(
        b"<img src=\"data:image/png;base64,AAAA\">",
        "script-url"
    ));
}

#[test]
fn mutation_on_reparse_is_an_exploit() {
    assert!(
        has(
            b"<svg></p><style><a id=\"</style><img src=1 onerror=alert(1)>\">",
            "event-handler"
        ) || has(
            b"<svg></p><style><a id=\"</style><img src=1 onerror=alert(1)>\">",
            "mxss"
        )
    );
}

#[test]
fn surviving_declarations_are_exploits() {
    assert!(has(
        b"<!DOCTYPE x [<!ENTITY a \"b\">]><p>&a;</p>",
        "declaration"
    ));
    assert!(has(
        b"<?xml-stylesheet href=\"x.xsl\"?><p>x</p>",
        "declaration"
    ));
    assert!(has(b"<p>&lol9;</p>", "unknown-entity"));
}

#[test]
fn svg_href_rewrites_are_exploits() {
    assert!(has(
        b"<svg><a><animate attributeName=\"href\" values=\"javascript:alert(1)\"/></a></svg>",
        "element"
    ));
    assert!(has(
        b"<svg><a xlink:href=\"javascript:alert(1)\">x</a></svg>",
        "script-url"
    ));
}

#[test]
fn odd_schemes_and_inert_remnants_are_policy() {
    let findings = inspect(
        b"<a href=\"foo:bar\">x</a><!doctype x [>",
        &allowed(),
        &protocols(),
        false,
    );
    assert!(
        findings.iter().all(|f| f.level == Level::Policy),
        "{findings:?}"
    );
    assert!(findings.iter().any(|f| f.rule == "unlisted-scheme"));
    assert!(findings.iter().any(|f| f.rule == "inert-declaration"));
}

#[test]
fn clean_post_has_no_findings() {
    let post = b"<p class=\"x\">Hello <a href=\"https://example.org/?a=1&amp;b=2\" title=\"t\">link</a> &amp; <img src=\"/a.png\" style=\"color: red\"></p><!-- c -->";
    assert_eq!(inspect(post, &allowed(), &protocols(), false), vec![]);
}

#[test]
fn bombs_do_not_hang_the_oracle() {
    let laughs = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../speedyspec-wp-kses-testkit/corpus/bombs/billion-laughs.xml"
    ))
    .unwrap();
    let start = std::time::Instant::now();
    let _ = inspect(&laughs, &allowed(), &protocols(), true);
    assert!(start.elapsed().as_secs() < 2);
}
