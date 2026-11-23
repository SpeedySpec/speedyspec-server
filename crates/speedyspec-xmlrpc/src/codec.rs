//! The XML-RPC codec (Adapter): `methodCall` and `methodResponse` documents to and from the neutral [`Value`].
//!
//! Reading is defensive, as the request body is the client's: no DTD is ever honored (a `<!DOCTYPE` is refused, so no
//! entity is expanded and nothing external is read), the element count is bounded as IXR bounds it, and nesting is
//! bounded so a deep document cannot exhaust the stack. Every failure is IXR's parse fault, with no detail about
//! the input.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{Duration, NaiveDateTime};
use indexmap::IndexMap;
use quick_xml::Reader;
use quick_xml::events::Event;
use speedyspec_operation_contracts::Value;
use speedyspec_xmlrpc_contracts::{Fault, MethodCall, MethodResponse};

/// The deepest value nesting a request may have.
pub const MAX_VALUE_DEPTH: usize = 256;

/// Elements per nesting level: `value > array > data` or `value > struct > member`.
const ELEMENTS_PER_LEVEL: usize = 3;

const HEADER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";

/// IXR's fault for a document that cannot be read.
pub fn not_well_formed() -> Fault {
    Fault::new(-32700, "parse error. not well formed")
}

/// IXR's fault for a well-formed document that is not a `methodCall`.
pub fn not_a_call() -> Fault {
    Fault::new(
        -32600,
        "server error. invalid xml-rpc. not conforming to spec. Request must be a methodCall",
    )
}

/// One element read from the document.
#[derive(Default)]
struct Node {
    name: String,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
}

/// The predefined XML entities, the only named references honored.
fn predefined(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => return None,
    })
}

/// The document's root element, read without recursion.
fn read_tree(body: &[u8], element_limit: usize) -> Result<Node, Fault> {
    let text = std::str::from_utf8(body).map_err(|_| not_well_formed())?;
    // IXR's bound: twice the element limit in `<` characters.
    if element_limit > 0 && text.bytes().filter(|b| *b == b'<').count() > 2 * element_limit {
        return Err(not_well_formed());
    }
    let max_depth = MAX_VALUE_DEPTH * ELEMENTS_PER_LEVEL + 8;
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<Node> = vec![Node::default()];
    loop {
        let event = reader.read_event().map_err(|_| not_well_formed())?;
        match event {
            Event::Start(start) => {
                if stack.len() > max_depth {
                    return Err(not_well_formed());
                }
                let name = start.name().as_ref().to_owned();
                stack.push(Node {
                    name,
                    ..Node::default()
                });
            }
            Event::Empty(start) => {
                let name = start.name().as_ref().to_owned();
                stack.last_mut().expect("root").children.push(Node {
                    name,
                    ..Node::default()
                });
            }
            Event::End(end) => {
                let node = stack.pop().expect("open element");
                if stack.is_empty() || node.name != end.name().as_ref() {
                    return Err(not_well_formed());
                }
                stack.last_mut().expect("parent").children.push(node);
            }
            Event::Text(t) => stack.last_mut().expect("root").text.push_str(&t),
            Event::CData(c) => stack.last_mut().expect("root").text.push_str(&c),
            Event::GeneralRef(r) => {
                let resolved = if r.is_char_ref() {
                    r.resolve_char_ref().ok().flatten()
                } else {
                    predefined(&r)
                };
                let c = resolved.ok_or_else(not_well_formed)?;
                stack.last_mut().expect("root").text.push(c);
            }
            Event::DocType(_) => return Err(not_well_formed()),
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) => {}
            Event::Eof => break,
        }
    }
    if stack.len() != 1 {
        return Err(not_well_formed());
    }
    let mut document = stack.pop().expect("root");
    let mut roots = document.children.drain(..);
    match (roots.next(), roots.next()) {
        (Some(root), None) => Ok(root),
        _ => Err(not_well_formed()),
    }
}

/// A date in any accepted form, normalized to UTC.
fn parse_date(text: &str) -> Option<NaiveDateTime> {
    let text = text.trim();
    let (local, offset_minutes) = if let Some(local) = text.strip_suffix('Z') {
        (local, 0)
    } else {
        match text.rfind(['+', '-']).filter(|i| *i > 10) {
            Some(i) => {
                let sign = if text.as_bytes()[i] == b'-' { -1 } else { 1 };
                let digits: String = text[i + 1..].chars().filter(|c| *c != ':').collect();
                if digits.len() != 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
                    return None;
                }
                let hours: i64 = digits[..2].parse().ok()?;
                let minutes: i64 = digits[2..].parse().ok()?;
                (&text[..i], sign * (hours * 60 + minutes))
            }
            None => (text, 0),
        }
    };
    let parsed = ["%Y%m%dT%H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y%m%dT%H%M%S"]
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(local, format).ok())?;
    Some(parsed - Duration::minutes(offset_minutes))
}

/// The value a `<value>` element holds.
fn to_value(node: &Node) -> Result<Value, Fault> {
    let Some(typed) = node.children.first() else {
        // A bare `<value>text</value>` is a string, whitespace kept.
        return Ok(Value::String(node.text.clone()));
    };
    if node.children.len() > 1 || !node.text.trim().is_empty() {
        return Err(not_well_formed());
    }
    let text = typed.text.as_str();
    Ok(match typed.name.as_str() {
        "int" | "i4" | "i8" => Value::Int(text.trim().parse().map_err(|_| not_well_formed())?),
        "double" => Value::Float(text.trim().parse().map_err(|_| not_well_formed())?),
        "boolean" => match text.trim() {
            "1" | "true" => Value::Bool(true),
            "0" | "false" => Value::Bool(false),
            _ => return Err(not_well_formed()),
        },
        "string" => Value::String(text.to_owned()),
        "dateTime.iso8601" => Value::DateTime(parse_date(text).ok_or_else(not_well_formed)?),
        "base64" => {
            let compact: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
            Value::Binary(STANDARD.decode(compact).map_err(|_| not_well_formed())?)
        }
        "nil" => Value::Null,
        "struct" => {
            let mut map = IndexMap::new();
            for member in &typed.children {
                if member.name != "member" {
                    return Err(not_well_formed());
                }
                let name = member.child("name").ok_or_else(not_well_formed)?;
                let value = member.child("value").ok_or_else(not_well_formed)?;
                map.insert(name.text.clone(), to_value(value)?);
            }
            Value::Map(map)
        }
        "array" => {
            let data = typed.child("data").ok_or_else(not_well_formed)?;
            let items = data
                .children
                .iter()
                .map(|item| {
                    if item.name == "value" {
                        to_value(item)
                    } else {
                        Err(not_well_formed())
                    }
                })
                .collect::<Result<_, _>>()?;
            Value::List(items)
        }
        _ => return Err(not_well_formed()),
    })
}

/// The values of a `<params>` element.
fn params(node: Option<&Node>) -> Result<Vec<Value>, Fault> {
    let Some(params) = node else {
        return Ok(Vec::new());
    };
    params
        .children
        .iter()
        .map(|param| {
            let value = param
                .child("value")
                .filter(|_| param.name == "param")
                .ok_or_else(not_well_formed)?;
            to_value(value)
        })
        .collect()
}

/// Reads a `methodCall`. `element_limit` is WordPress's `xmlrpc_element_limit` (0 for none).
pub fn parse_call(body: &[u8], element_limit: usize) -> Result<MethodCall, Fault> {
    let root = read_tree(body, element_limit)?;
    if root.name != "methodCall" {
        return Err(not_a_call());
    }
    let method = root
        .child("methodName")
        .ok_or_else(not_a_call)?
        .text
        .trim()
        .to_owned();
    Ok(MethodCall {
        method,
        params: params(root.child("params"))?,
    })
}

/// Reads a `methodResponse`.
pub fn parse_response(body: &[u8]) -> Result<MethodResponse, Fault> {
    let root = read_tree(body, 0)?;
    if root.name != "methodResponse" {
        return Err(not_well_formed());
    }
    if let Some(fault) = root.child("fault") {
        let value = to_value(fault.child("value").ok_or_else(not_well_formed)?)?;
        let code = match value.get("faultCode") {
            Some(Value::Int(code)) => *code,
            _ => return Err(not_well_formed()),
        };
        let message = match value.get("faultString") {
            Some(Value::String(message)) => message.clone(),
            _ => String::new(),
        };
        return Ok(MethodResponse::Fault(Fault::new(code, message)));
    }
    let mut values = params(root.child("params"))?;
    if values.len() != 1 {
        return Err(not_well_formed());
    }
    Ok(MethodResponse::Success(values.remove(0)))
}

fn escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

/// `value`'s typed element, as IXR writes it (`null` is an empty string, every integer an `<int>`).
fn write_value(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("<string></string>"),
        Value::Bool(b) => out.push_str(if *b {
            "<boolean>1</boolean>"
        } else {
            "<boolean>0</boolean>"
        }),
        Value::Int(i) => out.push_str(&format!("<int>{i}</int>")),
        Value::Float(f) => out.push_str(&format!("<double>{f}</double>")),
        Value::String(s) => {
            out.push_str("<string>");
            escape(s, out);
            out.push_str("</string>");
        }
        Value::Binary(bytes) => {
            out.push_str("<base64>");
            out.push_str(&STANDARD.encode(bytes));
            out.push_str("</base64>");
        }
        Value::DateTime(at) => {
            out.push_str("<dateTime.iso8601>");
            out.push_str(&at.format("%Y%m%dT%H:%M:%S").to_string());
            out.push_str("</dateTime.iso8601>");
        }
        Value::List(items) => {
            out.push_str("<array><data>");
            for item in items {
                out.push_str("<value>");
                write_value(item, out);
                out.push_str("</value>");
            }
            out.push_str("</data></array>");
        }
        Value::Map(map) => {
            out.push_str("<struct>");
            for (name, item) in map {
                out.push_str("<member><name>");
                escape(name, out);
                out.push_str("</name><value>");
                write_value(item, out);
                out.push_str("</value></member>");
            }
            out.push_str("</struct>");
        }
    }
}

/// Writes a `methodResponse`, with the XML header.
pub fn write_response(response: &MethodResponse) -> Vec<u8> {
    let mut out = String::from(HEADER);
    out.push_str("<methodResponse>");
    match response {
        MethodResponse::Success(value) => {
            out.push_str("<params><param><value>");
            write_value(value, &mut out);
            out.push_str("</value></param></params>");
        }
        MethodResponse::Fault(fault) => {
            out.push_str("<fault><value>");
            let value = Value::map([
                ("faultCode", Value::Int(fault.code)),
                ("faultString", Value::String(fault.message.clone())),
            ]);
            write_value(&value, &mut out);
            out.push_str("</value></fault>");
        }
    }
    out.push_str("</methodResponse>\n");
    out.into_bytes()
}

/// Writes a `methodCall`, with the XML header.
pub fn write_call(call: &MethodCall) -> Vec<u8> {
    let mut out = String::from(HEADER);
    out.push_str("<methodCall><methodName>");
    escape(&call.method, &mut out);
    out.push_str("</methodName><params>");
    for param in &call.params {
        out.push_str("<param><value>");
        write_value(param, &mut out);
        out.push_str("</value></param>");
    }
    out.push_str("</params></methodCall>\n");
    out.into_bytes()
}
