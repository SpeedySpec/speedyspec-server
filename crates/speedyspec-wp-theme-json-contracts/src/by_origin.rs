use crate::Origin;

/// At most one value per origin, always iterated in WordPress's origin order.
///
/// WordPress keys presets and the spacing scale by origin after construction (`{ "theme": [...] }`).
#[derive(Clone, Debug, PartialEq)]
pub struct ByOrigin<T> {
    slots: [Option<T>; 4],
}

impl<T> Default for ByOrigin<T> {
    fn default() -> Self {
        Self {
            slots: [None, None, None, None],
        }
    }
}

impl<T> ByOrigin<T> {
    fn index(origin: Origin) -> usize {
        Origin::ALL.iter().position(|o| *o == origin).unwrap_or(2)
    }

    /// The value for `origin`, if there is one.
    pub fn get(&self, origin: Origin) -> Option<&T> {
        self.slots[Self::index(origin)].as_ref()
    }

    /// A mutable reference to the value for `origin`, if there is one.
    pub fn get_mut(&mut self, origin: Origin) -> Option<&mut T> {
        self.slots[Self::index(origin)].as_mut()
    }

    /// Sets the value for `origin`, replacing any earlier one.
    pub fn set(&mut self, origin: Origin, value: T) {
        self.slots[Self::index(origin)] = Some(value);
    }

    /// Removes and returns the value for `origin`.
    pub fn take(&mut self, origin: Origin) -> Option<T> {
        self.slots[Self::index(origin)].take()
    }

    /// Whether no origin has a value.
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }

    /// The values that are present, in origin order.
    pub fn iter(&self) -> impl Iterator<Item = (Origin, &T)> {
        Origin::ALL
            .into_iter()
            .zip(self.slots.iter())
            .filter_map(|(o, v)| v.as_ref().map(|v| (o, v)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Origin;

    #[test]
    fn by_origin_iterates_in_origin_order() {
        let mut by = ByOrigin::default();
        by.set(Origin::Custom, "c");
        by.set(Origin::Default, "d");
        let seen: Vec<_> = by.iter().map(|(o, v)| (o, *v)).collect();
        assert_eq!(seen, vec![(Origin::Default, "d"), (Origin::Custom, "c")]);
        assert_eq!(by.get(Origin::Theme), None);
    }
}
