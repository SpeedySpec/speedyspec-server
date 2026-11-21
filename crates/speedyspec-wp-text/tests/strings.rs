//! PHP's string functions with the semantics WordPress relies on.

use speedyspec_hooks::Value;
use speedyspec_wp_text::strings::{StrReplace, StrtrTable, strtr, to_php_string};

fn pairs(list: &[(&str, &str)]) -> Vec<(Vec<u8>, Vec<u8>)> {
    list.iter().map(|(a, b)| (a.as_bytes().to_vec(), b.as_bytes().to_vec())).collect()
}

#[test]
fn str_replace_is_sequential() {
    assert_eq!(&*StrReplace::new(pairs(&[("bc", "Y"), ("ab", "X")])).apply(b"abc"), b"aY");
    assert_eq!(&*StrReplace::new(pairs(&[("a", "b"), ("b", "c")])).apply(b"a"), b"c");
}

#[test]
fn str_replace_one_pass_when_safe() {
    let table = pairs(&[("&quot;", "&#34;"), ("&amp;", "&#38;"), ("&lt;", "&#60;"), ("&gt;", "&#62;"), ("&nbsp;", "&#160;")]);
    let text = b"&quot;&amp;&lt;&gt;&amp;lt;";
    let mut expected = text.to_vec();
    for (from, to) in &table {
        expected = StrReplace::new(vec![(from.clone(), to.clone())]).apply(&expected).into_owned();
    }
    assert_eq!(&*StrReplace::new(table).apply(text), &expected[..]);
}

#[test]
fn strtr_longest_first() {
    let table = StrtrTable::new(pairs(&[("a", "1"), ("ab", "2")]));
    assert_eq!(&*strtr(b"abab a", &table), b"22 1");
    let table = StrtrTable::new(pairs(&[("a", "b"), ("b", "c")]));
    assert_eq!(&*strtr(b"ab", &table), b"bc");
}

#[test]
fn to_php_string_scalars() {
    assert_eq!(to_php_string(&Value::Int(-3)).unwrap(), b"-3");
    assert_eq!(to_php_string(&Value::Float(1.0)).unwrap(), b"1");
    assert_eq!(to_php_string(&Value::Float(0.1)).unwrap(), b"0.1");
    assert_eq!(to_php_string(&Value::Float(1e25)).unwrap(), b"1.0E+25");
    assert_eq!(to_php_string(&Value::Float(-1.5e-7)).unwrap(), b"-1.5E-7");
    assert_eq!(to_php_string(&Value::Float(f64::INFINITY)).unwrap(), b"INF");
    assert_eq!(to_php_string(&Value::Float(f64::NAN)).unwrap(), b"NAN");
    assert_eq!(to_php_string(&Value::Bool(true)).unwrap(), b"1");
    assert_eq!(to_php_string(&Value::Bool(false)).unwrap(), b"");
    assert_eq!(to_php_string(&Value::Null).unwrap(), b"");
    assert_eq!(to_php_string(&Value::Str("é".into())).unwrap(), "é".as_bytes());
    assert_eq!(to_php_string(&Value::Array(Vec::new())), None);
}
