//! `_links` and `_embed`, from type relations.

use indexmap::IndexMap;
use speedyspec_operation_contracts::{Action, OperationCatalog, TypeCatalog, Value};
use speedyspec_rest_contracts::{RestLink, RestMethod, RestResponse};

use crate::RouteTable;
use crate::transport::route_parameters;

/// A value as it appears in a URL.
fn url_text(value: &Value) -> Option<String> {
    match value {
        Value::Int(i) => Some(i.to_string()),
        Value::Float(f) => Some(f.to_string()),
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(if *b { "1" } else { "0" }.to_owned()),
        _ => None,
    }
}

/// Percent-encodes a URL component.
fn encode_component(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// `route` with each named group replaced by its value; `None` when a value is missing.
fn fill(route: &str, values: &IndexMap<String, String>) -> Option<String> {
    let mut out = String::new();
    let mut rest = route;
    while let Some(start) = rest.find("(?P<").or_else(|| rest.find("(?<")) {
        out.push_str(&rest[..start]);
        let group = &rest[start..];
        let name_start = group.find('<')? + 1;
        let name_end = group.find('>')?;
        let name = &group[name_start..name_end];
        let mut depth = 0;
        let mut end = None;
        for (i, c) in group.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push_str(&encode_component(values.get(name)?));
        rest = &group[end? + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// The href of `operation`'s GET route with `args` (operation input → value) filled in: route parameters in the path,
/// the rest in the query string.
fn href(
    table: &RouteTable,
    operation: &str,
    args: &IndexMap<String, String>,
    base_url: &str,
) -> Option<String> {
    let entry = table
        .routes()
        .into_iter()
        .find(|e| e.operation == operation && e.methods.contains(&RestMethod::Get))?;
    let in_route = route_parameters(&entry.route);
    let path = fill(&entry.route, args)?;
    let query: Vec<String> = args
        .iter()
        .filter(|(k, _)| !in_route.contains(k))
        .map(|(k, v)| format!("{}={}", encode_component(k), encode_component(v)))
        .collect();
    let base = base_url.trim_end_matches('/');
    Some(if query.is_empty() {
        format!("{base}{path}")
    } else {
        format!("{base}{path}?{}", query.join("&"))
    })
}

/// The links of one item of type `type_name`: one embeddable link per relation whose target has a GET route and whose
/// source fields are present, and `self` when the type has a `get` operation with a route.
pub fn for_item(
    type_name: &str,
    item: &Value,
    types: &dyn TypeCatalog,
    operations: &dyn OperationCatalog,
    table: &RouteTable,
    base_url: &str,
) -> Vec<RestLink> {
    let mut links = Vec::new();
    let Some(spec) = types.get_type(type_name) else {
        return links;
    };
    for name in operations.operation_names() {
        let Some(op) = operations.get_operation(&name) else {
            continue;
        };
        let Some(resource) = op
            .resource
            .as_ref()
            .filter(|r| r.type_name == type_name && r.action == Action::Get)
        else {
            continue;
        };
        let args: Option<IndexMap<String, String>> = resource
            .identity
            .iter()
            .map(|field| {
                item.get(field)
                    .and_then(url_text)
                    .map(|v| (field.clone(), v))
            })
            .collect();
        if let Some(href) = args.and_then(|args| href(table, &name, &args, base_url)) {
            links.push(RestLink {
                rel: "self".into(),
                href,
                embeddable: false,
            });
            break;
        }
    }
    for relation in &spec.relations {
        let args: Option<IndexMap<String, String>> = relation
            .args
            .iter()
            .map(|(input, source)| {
                item.get(source)
                    .and_then(url_text)
                    .map(|v| (input.clone(), v))
            })
            .collect();
        if let Some(href) = args.and_then(|args| href(table, &relation.operation, &args, base_url))
        {
            links.push(RestLink {
                rel: relation.field.clone(),
                href,
                embeddable: true,
            });
        }
    }
    links
}

/// Writes `response.links` into its data as `_links` (`{rel: [{href, embeddable?}]}`). Data that is not an object is
/// left alone.
pub fn render(response: &mut RestResponse) {
    if response.links.is_empty() {
        return;
    }
    let Value::Map(data) = &mut response.data else {
        return;
    };
    let mut rendered: IndexMap<String, Value> = IndexMap::new();
    for link in &response.links {
        let mut entry = vec![("href", Value::from(link.href.as_str()))];
        if link.embeddable {
            entry.push(("embeddable", Value::Bool(true)));
        }
        let slot = rendered
            .entry(link.rel.clone())
            .or_insert_with(|| Value::List(Vec::new()));
        if let Value::List(list) = slot {
            list.push(Value::map(entry));
        }
    }
    data.insert("_links".into(), Value::Map(rendered));
}

/// Renders the links, then embeds each embeddable one under `_embedded[rel]`: `dispatch(href)`'s data, which for an
/// error is its error body, as WordPress embeds. Embedded responses are not embedded again.
pub fn embed(response: &mut RestResponse, dispatch: &dyn Fn(&str) -> RestResponse) {
    embed_only(response, dispatch, None);
}

/// As [`embed`], limited to the relations in `only` when given (`?_embed=author,comments`).
pub fn embed_only(
    response: &mut RestResponse,
    dispatch: &dyn Fn(&str) -> RestResponse,
    only: Option<&[String]>,
) {
    render(response);
    let embeddable: Vec<RestLink> = response
        .links
        .iter()
        .filter(|l| l.embeddable && only.is_none_or(|rels| rels.contains(&l.rel)))
        .cloned()
        .collect();
    if embeddable.is_empty() {
        return;
    }
    let mut embedded: IndexMap<String, Value> = IndexMap::new();
    for link in embeddable {
        let answer = dispatch(&link.href);
        let slot = embedded
            .entry(link.rel.clone())
            .or_insert_with(|| Value::List(Vec::new()));
        if let Value::List(list) = slot {
            list.push(answer.data);
        }
    }
    if let Value::Map(data) = &mut response.data {
        data.insert("_embedded".into(), Value::Map(embedded));
    }
}
