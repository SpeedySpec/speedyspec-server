use speedyspec_http::Method;

use crate::{RouteId, RouteName, Rule};

/// The version of [`RouteData::to_bytes`]'s format. Data of another version is a miss and is rebuilt.
pub const ROUTE_DATA_VERSION: u32 = 1;

/// A route with no parameters, found by one hash lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticRoute {
    pub method: Method,
    pub path: String,
    pub id: RouteId,
}

/// A route with parameters, matched by regex in registration order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicRoute {
    pub methods: Vec<Method>,
    pub regex: String,
    pub params: Vec<String>,
    pub id: RouteId,
}

/// Everything a route table is compiled from, as plain data with no handlers, so it can be cached by fingerprint
/// and handed to stores (including PHP ones) as bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteData {
    pub statics: Vec<StaticRoute>,
    pub dynamics: Vec<DynamicRoute>,
    pub before_rules: Vec<Rule>,
    pub after_rules: Vec<Rule>,
    pub names: Vec<(RouteName, RouteId)>,
}

impl RouteData {
    /// A versioned binary form: the version, then length-prefixed fields.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer(Vec::new());
        w.u32(ROUTE_DATA_VERSION);
        w.u32(self.statics.len() as u32);
        for route in &self.statics {
            w.str(route.method.as_str());
            w.str(&route.path);
            w.u32(route.id.0);
        }
        w.u32(self.dynamics.len() as u32);
        for route in &self.dynamics {
            w.u32(route.methods.len() as u32);
            for method in &route.methods {
                w.str(method.as_str());
            }
            w.str(&route.regex);
            w.u32(route.params.len() as u32);
            for param in &route.params {
                w.str(param);
            }
            w.u32(route.id.0);
        }
        for rules in [&self.before_rules, &self.after_rules] {
            w.u32(rules.len() as u32);
            for rule in rules {
                w.str(&rule.pattern);
                w.str(&rule.query);
            }
        }
        w.u32(self.names.len() as u32);
        for (name, id) in &self.names {
            w.str(name.as_str());
            w.u32(id.0);
        }
        w.0
    }

    /// Reads [`RouteData::to_bytes`]'s form; `None` for another version or any malformed input.
    pub fn from_bytes(bytes: &[u8]) -> Option<RouteData> {
        let mut r = Reader { bytes, at: 0 };
        if r.u32()? != ROUTE_DATA_VERSION {
            return None;
        }
        let mut data = RouteData::default();
        for _ in 0..r.count()? {
            let method = Method::parse(&r.str()?);
            let path = r.str()?;
            data.statics.push(StaticRoute {
                method,
                path,
                id: RouteId(r.u32()?),
            });
        }
        for _ in 0..r.count()? {
            let mut methods = Vec::new();
            for _ in 0..r.count()? {
                methods.push(Method::parse(&r.str()?));
            }
            let regex = r.str()?;
            let mut params = Vec::new();
            for _ in 0..r.count()? {
                params.push(r.str()?);
            }
            data.dynamics.push(DynamicRoute {
                methods,
                regex,
                params,
                id: RouteId(r.u32()?),
            });
        }
        for slot in 0..2 {
            let mut rules = Vec::new();
            for _ in 0..r.count()? {
                let pattern = r.str()?;
                rules.push(Rule {
                    pattern,
                    query: r.str()?,
                });
            }
            if slot == 0 {
                data.before_rules = rules;
            } else {
                data.after_rules = rules;
            }
        }
        for _ in 0..r.count()? {
            let name = RouteName::new(&r.str()?);
            data.names.push((name, RouteId(r.u32()?)));
        }
        (r.at == bytes.len()).then_some(data)
    }
}

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn str(&mut self, value: &str) {
        self.u32(value.len() as u32);
        self.0.extend_from_slice(value.as_bytes());
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> Option<u32> {
        let end = self.at.checked_add(4)?;
        let value = u32::from_le_bytes(self.bytes.get(self.at..end)?.try_into().ok()?);
        self.at = end;
        Some(value)
    }

    /// A count, refused when it could not fit in the remaining bytes (each item takes at least 4).
    fn count(&mut self) -> Option<u32> {
        let count = self.u32()?;
        (count as usize <= (self.bytes.len() - self.at) / 4 + 1).then_some(count)
    }

    fn str(&mut self) -> Option<String> {
        let len = self.u32()? as usize;
        let end = self.at.checked_add(len)?;
        let value = std::str::from_utf8(self.bytes.get(self.at..end)?)
            .ok()?
            .to_string();
        self.at = end;
        Some(value)
    }
}
