//! Attribute cases, each expected value taken from WordPress's `shortcode_parse_atts()` on PHP 8.5.

use speedyspec_wp_shortcode_contracts::AttKey;

/// One attribute text, the array WordPress returns, and whether PHP raised the null-subject deprecation.
pub struct AttsCase {
    pub text: &'static [u8],
    pub expected: Vec<(AttKey, Vec<u8>)>,
    pub null_subject: bool,
}

pub fn atts_cases() -> Vec<AttsCase> {
    vec![
        AttsCase { text: b"a=\x221\x22 b='2' c=3 \x22d\x22 'e' f", expected: vec![(AttKey::Str(b"a".to_vec()), b"1".to_vec()), (AttKey::Str(b"b".to_vec()), b"2".to_vec()), (AttKey::Str(b"c".to_vec()), b"3".to_vec()), (AttKey::Int(0), b"d".to_vec()), (AttKey::Int(1), b"e".to_vec()), (AttKey::Int(2), b"f".to_vec())], null_subject: false },
        AttsCase { text: b"A=\x22x\x22", expected: vec![(AttKey::Str(b"a".to_vec()), b"x".to_vec())], null_subject: false },
        AttsCase { text: b"1=\x22x\x22", expected: vec![(AttKey::Int(1), b"x".to_vec())], null_subject: false },
        AttsCase { text: b"0=\x22x\x22", expected: vec![], null_subject: false },
        AttsCase { text: b"0='x'", expected: vec![], null_subject: false },
        AttsCase { text: b"0=x", expected: vec![], null_subject: false },
        AttsCase { text: b"\x22\x22 ''", expected: vec![], null_subject: false },
        AttsCase { text: b"a=\x221\x22 a=\x222\x22", expected: vec![(AttKey::Str(b"a".to_vec()), b"2".to_vec())], null_subject: false },
        AttsCase { text: b"a=\x22<b>\x22", expected: vec![(AttKey::Str(b"a".to_vec()), b"<b>".to_vec())], null_subject: false },
        AttsCase { text: b"a=\x22<b\x22", expected: vec![(AttKey::Str(b"a".to_vec()), b"".to_vec())], null_subject: false },
        AttsCase { text: b"a=\x22\x5cx41\x5c101\x5cn\x22", expected: vec![(AttKey::Str(b"a".to_vec()), b"AA\x0a".to_vec())], null_subject: false },
        AttsCase { text: b"a=1\x0a", expected: vec![(AttKey::Str(b"a".to_vec()), b"1".to_vec())], null_subject: false },
        AttsCase { text: b"a=1\x0a\x0a", expected: vec![(AttKey::Str(b"a".to_vec()), b"1".to_vec())], null_subject: false },
        AttsCase { text: b"a=\x221\x22\xc2\xa0b=\x222\x22", expected: vec![(AttKey::Str(b"a".to_vec()), b"1".to_vec()), (AttKey::Str(b"b".to_vec()), b"2".to_vec())], null_subject: false },
        AttsCase { text: b"a\xe2\x80\x8bb", expected: vec![(AttKey::Int(0), b"a".to_vec()), (AttKey::Int(1), b"b".to_vec())], null_subject: false },
        AttsCase { text: b"\xff a=\x221\x22", expected: vec![], null_subject: true },
        AttsCase { text: b"", expected: vec![], null_subject: false },
        AttsCase { text: b" ", expected: vec![], null_subject: false },
        AttsCase { text: b"x", expected: vec![(AttKey::Int(0), b"x".to_vec())], null_subject: false },
        AttsCase { text: b" 9=\x22a\x22 foo", expected: vec![(AttKey::Int(9), b"a".to_vec()), (AttKey::Int(10), b"foo".to_vec())], null_subject: false },
        AttsCase { text: b"a = \x221\x22", expected: vec![(AttKey::Str(b"a".to_vec()), b"1".to_vec())], null_subject: false },
        AttsCase { text: b"a=\x221\x22b=\x222\x22", expected: vec![(AttKey::Int(0), b"a=\x221\x22b=\x222\x22".to_vec())], null_subject: false },
        AttsCase { text: b"a=1\x22", expected: vec![(AttKey::Int(0), b"a=1\x22".to_vec())], null_subject: false },
        AttsCase { text: b"\x22a b\x22 'c d'", expected: vec![(AttKey::Int(0), b"a b".to_vec()), (AttKey::Int(1), b"c d".to_vec())], null_subject: false },
        AttsCase { text: b"a-b=\x221\x22 c_d=2", expected: vec![(AttKey::Str(b"a-b".to_vec()), b"1".to_vec()), (AttKey::Str(b"c_d".to_vec()), b"2".to_vec())], null_subject: false },
        AttsCase { text: b"-=\x221\x22", expected: vec![(AttKey::Str(b"-".to_vec()), b"1".to_vec())], null_subject: false },
        AttsCase { text: b"a=\x22x\x5c\x5cy\x22 b=\x22\x5ct\x22", expected: vec![(AttKey::Str(b"a".to_vec()), b"x\x5cy".to_vec()), (AttKey::Str(b"b".to_vec()), b"\x09".to_vec())], null_subject: false },
        AttsCase { text: b"a=\x22\x5cq\x5c\x5c\x22", expected: vec![(AttKey::Str(b"a".to_vec()), b"q\x5c".to_vec())], null_subject: false },
        AttsCase { text: b"<b> x", expected: vec![(AttKey::Int(0), b"<b>".to_vec()), (AttKey::Int(1), b"x".to_vec())], null_subject: false },
        AttsCase { text: b"a=\x221\x22  \x22b\x22 ", expected: vec![(AttKey::Str(b"a".to_vec()), b"1".to_vec()), (AttKey::Int(0), b"b".to_vec())], null_subject: false },
        AttsCase { text: b"\x09a=1\x09b=2", expected: vec![(AttKey::Str(b"a".to_vec()), b"1".to_vec()), (AttKey::Str(b"b".to_vec()), b"2".to_vec())], null_subject: false },
        AttsCase { text: b"a=", expected: vec![(AttKey::Int(0), b"a=".to_vec())], null_subject: false },
        AttsCase { text: b"=x", expected: vec![(AttKey::Int(0), b"=x".to_vec())], null_subject: false },
        AttsCase { text: b"\x22unclosed", expected: vec![(AttKey::Int(0), b"\x22unclosed".to_vec())], null_subject: false },
        AttsCase { text: b"a=\x22b>c<d>\x22", expected: vec![(AttKey::Str(b"a".to_vec()), b"b>c<d>".to_vec())], null_subject: false },
        AttsCase { text: b"a=\x22&lt;\x22 b='<i>' c=<x>", expected: vec![(AttKey::Str(b"a".to_vec()), b"&lt;".to_vec()), (AttKey::Str(b"b".to_vec()), b"<i>".to_vec()), (AttKey::Str(b"c".to_vec()), b"<x>".to_vec())], null_subject: false },
        AttsCase { text: b" url=\x22http://a/b?c=d&e=f\x22 ", expected: vec![(AttKey::Str(b"url".to_vec()), b"http://a/b?c=d&e=f".to_vec())], null_subject: false },
        AttsCase { text: b"9223372036854775808=\x22a\x22", expected: vec![(AttKey::Str(b"9223372036854775808".to_vec()), b"a".to_vec())], null_subject: false },
        AttsCase { text: b"-5=x y", expected: vec![(AttKey::Int(-5), b"x".to_vec()), (AttKey::Int(-4), b"y".to_vec())], null_subject: false },
        AttsCase { text: b"\xc3\xbc=\x221\x22", expected: vec![(AttKey::Int(0), b"\xc3\xbc=\x221\x22".to_vec())], null_subject: false },
        AttsCase { text: b"a=\x221\x22 2 \x223\x22", expected: vec![(AttKey::Str(b"a".to_vec()), b"1".to_vec()), (AttKey::Int(0), b"2".to_vec()), (AttKey::Int(1), b"3".to_vec())], null_subject: false },
        AttsCase { text: b"a=1\x0b", expected: vec![(AttKey::Str(b"a".to_vec()), b"1".to_vec())], null_subject: false },
        AttsCase { text: b"a=1\x0c b=2", expected: vec![(AttKey::Str(b"a".to_vec()), b"1".to_vec()), (AttKey::Str(b"b".to_vec()), b"2".to_vec())], null_subject: false },
        AttsCase { text: b"A=1 a=2", expected: vec![(AttKey::Str(b"a".to_vec()), b"2".to_vec())], null_subject: false },
    ]
}
