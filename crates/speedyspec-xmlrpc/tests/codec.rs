use chrono::NaiveDate;
use speedyspec_operation_contracts::Value;
use speedyspec_xmlrpc::codec::{
    Dates, RAW_DATE, parse_call, parse_call_with, parse_response, write_call, write_response,
};
use speedyspec_xmlrpc_contracts::{Fault, MethodCall, MethodResponse};

const LIMIT: usize = 30000;

fn call(params: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?><methodCall><methodName>demo.test</methodName><params>{params}</params></methodCall>"
    )
}

fn param(value: &str) -> String {
    format!("<param><value>{value}</value></param>")
}

fn one(value: &str) -> Value {
    let parsed = parse_call(call(&param(value)).as_bytes(), LIMIT).expect("parses");
    assert_eq!(parsed.method, "demo.test");
    assert_eq!(parsed.params.len(), 1);
    parsed.params.into_iter().next().unwrap()
}

fn at(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> Value {
    Value::DateTime(
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, s)
            .unwrap(),
    )
}

fn parse_fault(body: &str) -> Fault {
    parse_call(body.as_bytes(), LIMIT).expect_err("a fault")
}

fn not_well_formed() -> Fault {
    Fault::new(-32700, "parse error. not well formed")
}

#[test]
fn parses_every_type() {
    assert_eq!(one("<int>42</int>"), Value::Int(42));
    assert_eq!(one("<i4>-7</i4>"), Value::Int(-7));
    assert_eq!(one("<i8>1099511627776</i8>"), Value::Int(1 << 40));
    assert_eq!(one("<double>1.5</double>"), Value::Float(1.5));
    assert_eq!(one("<boolean>1</boolean>"), Value::Bool(true));
    assert_eq!(one("<boolean>0</boolean>"), Value::Bool(false));
    assert_eq!(
        one("<string>a &amp; b &lt;c&gt; &#233;</string>"),
        Value::String("a & b <c> é".into())
    );
    assert_eq!(one("<string/>"), Value::String(String::new()));
    assert_eq!(
        one("<dateTime.iso8601>20261123T10:00:00</dateTime.iso8601>"),
        at(2026, 11, 23, 10, 0, 0)
    );
    assert_eq!(
        one("<base64>aGVsbG8=</base64>"),
        Value::Binary(b"hello".to_vec())
    );
    assert_eq!(one("<nil/>"), Value::Null);
    assert_eq!(
        one(
            "<struct><member><name>b</name><value><int>2</int></value></member><member><name>a</name><value>x</value></member></struct>"
        ),
        Value::map([("b", Value::Int(2)), ("a", Value::String("x".into()))])
    );
    assert_eq!(
        one(
            "<array><data><value><int>1</int></value><value><string>two</string></value></data></array>"
        ),
        Value::List(vec![Value::Int(1), Value::String("two".into())])
    );
}

#[test]
fn bare_value_text_is_a_string() {
    assert_eq!(one(" spaced text "), Value::String(" spaced text ".into()));
    assert_eq!(one(""), Value::String(String::new()));
}

#[test]
fn whitespace_around_a_typed_value_is_ignored() {
    assert_eq!(one("\n  <int> 5 </int>\n"), Value::Int(5));
}

#[test]
fn no_params_is_an_empty_list() {
    let parsed = parse_call(
        b"<methodCall><methodName>system.listMethods</methodName></methodCall>",
        LIMIT,
    )
    .unwrap();
    assert_eq!(parsed.method, "system.listMethods");
    assert!(parsed.params.is_empty());
}

#[test]
fn dates_in_every_accepted_form_normalize_to_utc() {
    let expected = at(2026, 11, 23, 10, 0, 0);
    for date in [
        "20261123T10:00:00",
        "2026-11-23T10:00:00",
        "2026-11-23T10:00:00Z",
        "20261123T12:00:00+02:00",
        "20261123T07:30:00-0230",
        "20261123T100000",
    ] {
        assert_eq!(
            one(&format!("<dateTime.iso8601>{date}</dateTime.iso8601>")),
            expected,
            "{date}"
        );
    }
    assert_eq!(
        parse_fault(&call(&param(
            "<dateTime.iso8601>yesterday</dateTime.iso8601>"
        ))),
        not_well_formed()
    );
}

#[test]
fn base64_with_line_breaks_decodes() {
    assert_eq!(
        one("<base64>aGVs\r\n bG8=\n</base64>"),
        Value::Binary(b"hello".to_vec())
    );
}

#[test]
fn malformed_scalars_are_parse_faults() {
    for value in [
        "<int>five</int>",
        "<double>x</double>",
        "<boolean>maybe</boolean>",
        "<base64>***</base64>",
    ] {
        assert_eq!(
            parse_fault(&call(&param(value))),
            not_well_formed(),
            "{value}"
        );
    }
}

#[test]
fn doctype_is_a_parse_fault() {
    let xxe = "<?xml version=\"1.0\"?><!DOCTYPE m [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><methodCall><methodName>&x;</methodName></methodCall>";
    let fault = parse_fault(xxe);
    assert_eq!(fault, not_well_formed());
    assert!(!fault.message.contains("root:"));
}

#[test]
fn an_undeclared_entity_is_a_parse_fault() {
    assert_eq!(
        parse_fault(&call(&param("<string>&lol;</string>"))),
        not_well_formed()
    );
}

#[test]
fn non_utf8_and_malformed_bodies_are_parse_faults() {
    let mut latin1 = call(&param("<string>x</string>")).into_bytes();
    latin1.insert(60, 0xE9);
    assert_eq!(parse_call(&latin1, LIMIT).unwrap_err(), not_well_formed());
    assert_eq!(
        parse_fault("<methodCall><methodName>a</methodCall>"),
        not_well_formed()
    );
    assert_eq!(parse_fault(""), not_well_formed());
}

#[test]
fn too_many_elements_is_a_parse_fault() {
    let values: String = (0..21).map(|_| "<a>".to_owned()).collect();
    let body = format!("<methodCall><methodName>x</methodName>{values}</methodCall>");
    assert_eq!(
        parse_call(body.as_bytes(), 10).unwrap_err(),
        not_well_formed()
    );
    let small = "<methodCall><methodName>x</methodName></methodCall>";
    assert!(parse_call(small.as_bytes(), 10).is_ok());
}

#[test]
fn deep_nesting_is_a_parse_fault() {
    let depth = 10_000;
    let open = "<array><data><value>".repeat(depth);
    let close = "</value></data></array>".repeat(depth);
    let body = call(&param(&format!("{open}<int>1</int>{close}")));
    assert_eq!(
        parse_call(body.as_bytes(), 0).unwrap_err(),
        not_well_formed()
    );
    let fine = 200;
    let body = call(&param(&format!(
        "{}<int>1</int>{}",
        "<array><data><value>".repeat(fine),
        "</value></data></array>".repeat(fine)
    )));
    assert!(parse_call(body.as_bytes(), 0).is_ok());
}

#[test]
fn a_method_response_is_not_a_call() {
    assert_eq!(
        parse_fault(
            "<methodResponse><params><param><value>1</value></param></params></methodResponse>"
        ),
        Fault::new(
            -32600,
            "server error. invalid xml-rpc. not conforming to spec. Request must be a methodCall"
        )
    );
}

#[test]
fn writes_ixr_shaped_responses() {
    let header = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
    let ok = |inner: &str| {
        format!(
            "{header}<methodResponse><params><param><value>{inner}</value></param></params></methodResponse>\n"
        )
    };
    let write = |v: Value| String::from_utf8(write_response(&MethodResponse::Success(v))).unwrap();
    assert_eq!(write(Value::Null), ok("<string></string>"));
    assert_eq!(write(Value::Int(1 << 40)), ok("<int>1099511627776</int>"));
    assert_eq!(write(Value::Bool(true)), ok("<boolean>1</boolean>"));
    assert_eq!(
        write(Value::String("a<&>b".into())),
        ok("<string>a&lt;&amp;&gt;b</string>")
    );
    assert_eq!(
        write(at(2026, 1, 2, 3, 4, 5)),
        ok("<dateTime.iso8601>20260102T03:04:05</dateTime.iso8601>")
    );
    assert_eq!(
        write(Value::Binary(b"hello".to_vec())),
        ok("<base64>aGVsbG8=</base64>")
    );
    assert_eq!(
        write(Value::map([("k", Value::List(vec![Value::Int(1)]))])),
        ok(
            "<struct><member><name>k</name><value><array><data><value><int>1</int></value></data></array></value></member></struct>"
        )
    );
    assert_eq!(
        String::from_utf8(write_response(&MethodResponse::Fault(Fault::new(
            403, "No & no"
        ))))
        .unwrap(),
        format!(
            "{header}<methodResponse><fault><value><struct><member><name>faultCode</name><value><int>403</int></value></member><member><name>faultString</name><value><string>No &amp; no</string></value></member></struct></value></fault></methodResponse>\n"
        )
    );
}

#[test]
fn round_trip() {
    let original = MethodCall {
        method: "acme.editReport".into(),
        params: vec![
            Value::Int(1),
            Value::Float(2.25),
            Value::Bool(false),
            Value::String("<x> & y".into()),
            at(2026, 11, 23, 10, 0, 0),
            Value::Binary(vec![0, 255, 7]),
            Value::List(vec![
                Value::Int(1),
                Value::map([("a", Value::String("b".into()))]),
            ]),
            Value::map([("nested", Value::map([("deep", Value::Bool(true))]))]),
        ],
    };
    assert_eq!(parse_call(&write_call(&original), LIMIT).unwrap(), original);
}

#[test]
fn responses_parse_back() {
    let fault = MethodResponse::Fault(Fault::new(-32601, "nope"));
    assert_eq!(parse_response(&write_response(&fault)).unwrap(), fault);
    let ok = MethodResponse::Success(Value::List(vec![Value::Int(3)]));
    assert_eq!(parse_response(&write_response(&ok)).unwrap(), ok);
}

#[test]
fn strings_names_and_method_names_are_trimmed_as_ixr_trims_them() {
    let parsed = parse_call(
        b"<methodCall><methodName> demo.test\n</methodName><params><param><value><string>\n  secret\n</string></value></param><param><value><struct><member><name> key </name><value><string> v </string></value></member></struct></value></param></params></methodCall>",
        LIMIT,
    )
    .unwrap();
    assert_eq!(parsed.method, "demo.test");
    assert_eq!(parsed.params[0], Value::String("secret".into()));
    assert_eq!(
        parsed.params[1],
        Value::map([("key", Value::String("v".into()))])
    );
}

#[test]
fn raw_dates_keep_the_clients_text() {
    let body = call(&param(
        "<dateTime.iso8601> 20240102T10:00:00+02:00 </dateTime.iso8601>",
    ));
    let raw = parse_call_with(body.as_bytes(), LIMIT, Dates::Raw).unwrap();
    assert_eq!(
        raw.params[0],
        Value::map([(RAW_DATE, Value::String("20240102T10:00:00+02:00".into()))])
    );
    let bad = call(&param("<dateTime.iso8601>never</dateTime.iso8601>"));
    assert_eq!(
        parse_call_with(bad.as_bytes(), LIMIT, Dates::Raw).unwrap_err(),
        not_well_formed()
    );
}
