//! The metadata an operation is published with as an ability.

use speedyspec_operation_contracts::Value;

/// Whether `name` is a WordPress ability name: `namespace/name`, lowercase letters, digits and dashes.
pub fn is_ability_name(name: &str) -> bool {
    let Some((namespace, rest)) = name.split_once('/') else {
        return false;
    };
    let part = |p: &str| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    part(namespace) && part(rest)
}

/// Whether `slug` is a WordPress ability category slug: lowercase words of letters and digits joined by single dashes.
pub fn is_category_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.split('-').all(|word| {
            !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// WordPress's ability annotations; `None` means the default mapping's (or WordPress's `null`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Annotations {
    pub readonly: Option<bool>,
    pub destructive: Option<bool>,
    pub idempotent: Option<bool>,
}

impl Annotations {
    const KEYS: [&'static str; 3] = ["readonly", "destructive", "idempotent"];

    fn to_value(&self) -> Value {
        let mut entries = Vec::new();
        for (key, value) in
            Self::KEYS
                .iter()
                .zip([self.readonly, self.destructive, self.idempotent])
        {
            if let Some(value) = value {
                entries.push((*key, Value::Bool(value)));
            }
        }
        Value::map(entries)
    }

    fn from_value(value: &Value) -> Result<Annotations, String> {
        let Value::Map(map) = value else {
            return Err("annotations are a map".into());
        };
        if let Some(unknown) = map.keys().find(|k| !Self::KEYS.contains(&k.as_str())) {
            return Err(format!("{unknown} is not an ability annotation"));
        }
        let flag = |key: &str| match map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(format!("the {key} annotation is a boolean")),
        };
        Ok(Annotations {
            readonly: flag("readonly")?,
            destructive: flag("destructive")?,
            idempotent: flag("idempotent")?,
        })
    }

    fn merge(self, defaults: Annotations) -> Annotations {
        Annotations {
            readonly: self.readonly.or(defaults.readonly),
            destructive: self.destructive.or(defaults.destructive),
            idempotent: self.idempotent.or(defaults.idempotent),
        }
    }
}

/// An operation's ability metadata. Every `None` (and a `Null` meta) means "the default mapping's".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AbilityBinding {
    pub label: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub show_in_rest: Option<bool>,
    pub public: Option<bool>,
    pub annotations: Annotations,
    /// Further meta keys, as a map.
    pub meta: Value,
}

impl AbilityBinding {
    /// The binding as the binding store keeps it; unset fields are left out.
    pub fn to_value(&self) -> Value {
        let mut entries: Vec<(&str, Value)> = Vec::new();
        for (key, value) in [
            ("label", &self.label),
            ("description", &self.description),
            ("category", &self.category),
        ] {
            if let Some(value) = value {
                entries.push((key, Value::String(value.clone())));
            }
        }
        for (key, value) in [("show_in_rest", self.show_in_rest), ("public", self.public)] {
            if let Some(value) = value {
                entries.push((key, Value::Bool(value)));
            }
        }
        if self.annotations != Annotations::default() {
            entries.push(("annotations", self.annotations.to_value()));
        }
        if self.meta != Value::Null {
            entries.push(("meta", self.meta.clone()));
        }
        Value::map(entries)
    }

    /// The binding a stored value holds; `Err` says what is wrong with it.
    pub fn from_value(value: &Value) -> Result<AbilityBinding, String> {
        let Value::Map(map) = value else {
            return Err("an ability binding is a map".into());
        };
        let text = |key: &str| match map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(format!("the ability's {key} is a string")),
        };
        let flag = |key: &str| match map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(format!("the ability's {key} is a boolean")),
        };
        let category = text("category")?;
        if let Some(slug) = &category
            && !is_category_slug(slug)
        {
            return Err(format!(
                "{slug:?} is not an ability category slug: lowercase letters, digits and single dashes"
            ));
        }
        let meta = match map.get("meta") {
            None | Some(Value::Null) => Value::Null,
            Some(meta @ Value::Map(_)) => meta.clone(),
            Some(_) => return Err("the ability's meta is a map".into()),
        };
        Ok(AbilityBinding {
            label: text("label")?,
            description: text("description")?,
            category,
            show_in_rest: flag("show_in_rest")?,
            public: flag("public")?,
            annotations: match map.get("annotations") {
                None | Some(Value::Null) => Annotations::default(),
                Some(value) => Annotations::from_value(value)?,
            },
            meta,
        })
    }

    /// This binding, with `defaults` filling what it leaves unset. Meta maps are merged, this binding's keys winning.
    pub fn merge(self, defaults: AbilityBinding) -> AbilityBinding {
        let meta = match (self.meta, defaults.meta) {
            (Value::Map(mine), Value::Map(mut theirs)) => {
                for (key, value) in mine {
                    theirs.insert(key, value);
                }
                Value::Map(theirs)
            }
            (Value::Null, theirs) => theirs,
            (mine, _) => mine,
        };
        AbilityBinding {
            label: self.label.or(defaults.label),
            description: self.description.or(defaults.description),
            category: self.category.or(defaults.category),
            show_in_rest: self.show_in_rest.or(defaults.show_in_rest),
            public: self.public.or(defaults.public),
            annotations: self.annotations.merge(defaults.annotations),
            meta,
        }
    }
}
