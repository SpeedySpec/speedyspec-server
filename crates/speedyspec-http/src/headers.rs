use crate::HttpError;

/// Message headers: names compared case-insensitively, the first spelling kept, values kept in order.
///
/// Names and values are checked when set, so a value can never smuggle a line break into a response.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers {
    entries: Vec<(String, Vec<String>)>,
}

impl Headers {
    pub fn new() -> Headers {
        Headers::default()
    }

    /// The values joined by `", "`, as PSR-7's `getHeaderLine`, or `None` when absent.
    pub fn get(&self, name: &str) -> Option<String> {
        self.position(name).map(|i| self.entries[i].1.join(", "))
    }

    /// Every value of the header, or an empty slice.
    pub fn get_all(&self, name: &str) -> &[String] {
        match self.position(name) {
            Some(i) => &self.entries[i].1,
            None => &[],
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.position(name).is_some()
    }

    /// Each header with its kept spelling, in the order first set.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.entries
            .iter()
            .map(|(name, values)| (name.as_str(), values.as_slice()))
    }

    /// Replaces the header's values.
    pub fn with(mut self, name: &str, values: Vec<String>) -> Result<Headers, HttpError> {
        check(name, &values)?;
        match self.position(name) {
            Some(i) => self.entries[i].1 = values,
            None => self.entries.push((name.to_string(), values)),
        }
        Ok(self)
    }

    /// Appends values to the header, adding it when absent.
    pub fn with_added(mut self, name: &str, values: Vec<String>) -> Result<Headers, HttpError> {
        check(name, &values)?;
        match self.position(name) {
            Some(i) => self.entries[i].1.extend(values),
            None => self.entries.push((name.to_string(), values)),
        }
        Ok(self)
    }

    pub fn without(mut self, name: &str) -> Headers {
        self.entries
            .retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
        self
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|(existing, _)| existing.eq_ignore_ascii_case(name))
    }
}

fn check(name: &str, values: &[String]) -> Result<(), HttpError> {
    if name.is_empty() || !name.bytes().all(is_token_byte) {
        return Err(HttpError::InvalidHeaderName(name.to_string()));
    }
    for value in values {
        if !value
            .bytes()
            .all(|b| b == b'\t' || (b >= 0x20 && b != 0x7f))
        {
            return Err(HttpError::InvalidHeaderValue(value.clone()));
        }
    }
    Ok(())
}

/// RFC 9110 `tchar`.
fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}
