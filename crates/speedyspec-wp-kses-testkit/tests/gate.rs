use speedyspec_wp_kses_testkit::{Verdict, gate};

fn finder(s: &[u8]) -> Option<String> {
    (memchr_find(s, b"<script")).then(|| "script".to_string())
}

fn memchr_find(h: &[u8], n: &[u8]) -> bool {
    h.windows(n.len()).any(|w| w == n)
}

#[test]
fn same_output_is_same() {
    assert_eq!(gate(b"a", b"a", None), Verdict::Same);
}

#[test]
fn any_difference_fails_without_an_oracle() {
    assert!(matches!(gate(b"a", b"b", None), Verdict::Fail(_)));
}

#[test]
fn a_fix_of_a_php_exploit_is_a_security_break() {
    assert_eq!(
        gate(b"<script>x", b"x", Some(&finder)),
        Verdict::SecurityBreak("script".into())
    );
}

#[test]
fn an_exploit_in_the_port_always_fails() {
    assert!(matches!(
        gate(b"x", b"<script>x", Some(&finder)),
        Verdict::Fail(_)
    ));
    assert!(matches!(
        gate(b"<script>a", b"<script>b", Some(&finder)),
        Verdict::Fail(_)
    ));
}

#[test]
fn a_harmless_difference_fails() {
    assert!(matches!(gate(b"a", b"b", Some(&finder)), Verdict::Fail(_)));
}
