use speedyspec_routing::Rule;

/// A PHP array with string keys, as WordPress builds rule lists: assigning an existing key overwrites its value in
/// place, and `array_merge` keeps the first array's positions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhpArray(pub Vec<(String, String)>);

impl PhpArray {
    pub fn new() -> PhpArray {
        PhpArray::default()
    }

    /// `$array[ $key ] = $value`.
    pub fn set(&mut self, key: String, value: String) {
        match self.0.iter_mut().find(|(existing, _)| *existing == key) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key, value)),
        }
    }

    /// `array( k1 => v1, k2 => v2 )`, where a repeated key overwrites.
    pub fn from_pairs(pairs: Vec<(String, String)>) -> PhpArray {
        let mut array = PhpArray::new();
        for (key, value) in pairs {
            array.set(key, value);
        }
        array
    }

    /// `array_merge( $self, $other )` for string keys.
    pub fn merge(mut self, other: PhpArray) -> PhpArray {
        for (key, value) in other.0 {
            self.set(key, value);
        }
        self
    }

    pub fn into_rules(self) -> Vec<Rule> {
        self.0
            .into_iter()
            .map(|(pattern, query)| Rule { pattern, query })
            .collect()
    }

    pub fn from_rules(rules: &[Rule]) -> PhpArray {
        PhpArray::from_pairs(
            rules
                .iter()
                .map(|r| (r.pattern.clone(), r.query.clone()))
                .collect(),
        )
    }
}
