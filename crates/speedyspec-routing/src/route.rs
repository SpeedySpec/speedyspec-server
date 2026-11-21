/// A route's position in registration order; stable for one set of registrations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RouteId(pub u32);

/// A route's name, used to generate its URL.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RouteName(String);

impl RouteName {
    pub fn new(name: &str) -> RouteName {
        RouteName(name.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The values a route's parameters captured, in pattern order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Params(pub Vec<(String, String)>);

impl Params {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// A SHA-256 digest of everything a route table is compiled from: equal fingerprints mean equal tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    /// Lowercase hex, for stores that key by text.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}
