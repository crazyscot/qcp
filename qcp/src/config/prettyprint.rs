//! Configuration file pretty-printing
// (c) 2024 Ross Younger

use super::Manager;
use crate::cli::styles::use_colours;

use figment::{Metadata, value::Value};
use heck::ToUpperCamelCase;
use serde::Deserialize;
use std::{
    collections::HashSet,
    fmt::{Debug, Display},
};
use struct_field_names_as_array::FieldNamesAsSlice;
use tabled::{
    Table, Tabled,
    settings::{Color, object::Rows},
};

/// Data type used when rendering the config table
#[derive(Tabled)]
struct PrettyConfig {
    field: String,
    value: String,
    source: String,
}

impl PrettyConfig {
    fn render_source(meta: Option<&Metadata>) -> String {
        if let Some(m) = meta {
            m.source
                .as_ref()
                .map_or_else(|| m.name.to_string(), figment::Source::to_string)
        } else {
            String::new()
        }
    }

    fn render_value(value: &Value) -> String {
        match value {
            Value::String(_tag, s) => s.clone(),
            Value::Char(_tag, c) => c.to_string(),
            Value::Bool(_tag, b) => b.to_string(),
            Value::Num(_tag, num) => {
                if let Some(i) = num.to_i128() {
                    i.to_string()
                } else if let Some(u) = num.to_u128() {
                    u.to_string()
                } else if let Some(ff) = num.to_f64() {
                    ff.to_string()
                } else {
                    todo!("unknown Num case");
                }
            }
            Value::Empty(_tag, _) => "<empty>".into(),
            Value::Dict(_tag, _dict) => todo!("dicts are not currently supported"),
            Value::Array(_tag, vec) => {
                format!(
                    "[{}]",
                    vec.iter()
                        .map(PrettyConfig::render_value)
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
        }
    }

    fn new<F: Into<String>>(field: F, value: &Value, meta: Option<&Metadata>) -> Self {
        Self {
            field: field.into(),
            value: PrettyConfig::render_value(value),
            source: PrettyConfig::render_source(meta),
        }
    }
}

/// Pretty-printing type wrapper to Manager
#[derive(Debug)]
pub struct DisplayAdapter<'a> {
    /// Data source
    source: &'a Manager,
    /// The fields we want to output. (If empty, outputs everything.)
    fields: HashSet<String>,
}

impl Manager {
    /// Creates a `DisplayAdapter` for this struct with the given options.
    ///
    /// # Returns
    /// An ephemeral structure implementing `Display`.
    #[must_use]
    pub fn to_display_adapter<'de, T>(&self) -> DisplayAdapter<'_>
    where
        T: Deserialize<'de> + FieldNamesAsSlice,
    {
        let mut fields = HashSet::<String>::new();
        fields.extend(T::FIELD_NAMES_AS_SLICE.iter().map(|s| String::from(*s)));
        DisplayAdapter {
            source: self,
            fields,
        }
    }
}

impl Display for DisplayAdapter<'_> {
    /// Formats the contents of this structure which are relevant to a given output type.
    ///
    /// N.B. This function uses CLI styling.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let data = &self.source.data;

        let mut output = Vec::<PrettyConfig>::new();
        // First line of the table is special
        let (host_string, host_colour) = if let Some(host) = &self.source.host {
            (host.clone(), Color::FG_GREEN)
        } else {
            ("* (globals)".into(), Color::FG_CYAN)
        };
        output.push(PrettyConfig {
            field: "(Remote host)".into(),
            value: host_string,
            source: String::new(),
        });

        let mut keys = self.fields.iter().collect::<Vec<_>>();
        keys.sort();

        for field in keys {
            if let Ok(value) = data.find_value(field) {
                let meta = data.get_metadata(value.tag());
                output.push(PrettyConfig::new(field.to_upper_camel_case(), &value, meta));
            }
        }
        let mut writable = Table::new(output);
        let _ = writable.with(crate::styles::TABLE_STYLE.clone());
        if use_colours() {
            let _ = writable.modify(Rows::one(1), host_colour);
        }
        write!(f, "{writable}")
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use figment::{
        Metadata,
        value::{Empty, Map, Num, Tag, Value},
    };
    use pretty_assertions::assert_eq;

    use super::PrettyConfig;
    use crate::{Configuration, config::Manager};

    #[test]
    fn pretty_print() {
        let mut mgr = Manager::new(None, false, false);
        mgr.apply_system_default();
        let display = mgr.to_display_adapter::<Configuration>();
        let s = format!("{display}");
        assert!(s.contains("field"));
        assert!(s.contains("value"));
        assert!(s.contains("source"));
        assert!(s.contains("(Remote host)"));
        assert!(s.contains("InitialCongestionWindow"));
    }
    #[test]
    fn for_host() {
        let mut mgr = Manager::new(Some("testhost"), false, false);
        mgr.apply_system_default();
        let display = mgr.to_display_adapter::<Configuration>();
        let s = format!("{display}");
        assert!(s.contains("field"));
        assert!(s.contains("testhost"));
    }
    #[test]
    fn config_meta() {
        assert!(PrettyConfig::render_source(None).is_empty());
        assert_eq!(
            PrettyConfig::render_source(Some(&Metadata::named("test"))),
            "test"
        );
    }

    #[test]
    fn config_types() {
        assert_eq!(
            PrettyConfig::render_value(&Value::String(Tag::Default, "test".into())),
            "test"
        );
        assert_eq!(
            PrettyConfig::render_value(&Value::Char(Tag::Default, 'a')),
            "a"
        );
        assert_eq!(
            PrettyConfig::render_value(&Value::Bool(Tag::Default, true)),
            "true"
        );
        assert_eq!(
            PrettyConfig::render_value(&Value::Num(Tag::Default, 1.into())),
            "1"
        );
        assert_eq!(
            PrettyConfig::render_value(&Value::Num(Tag::Default, Num::I8(-1))),
            "-1"
        );
        assert_eq!(
            PrettyConfig::render_value(&Value::Num(Tag::Default, 0.5.into())),
            "0.5"
        );
        assert_eq!(
            PrettyConfig::render_value(&Value::Empty(Tag::Default, Empty::None)),
            "<empty>"
        );
        assert_eq!(
            PrettyConfig::render_value(&Value::Array(Tag::Default, vec![])),
            "[]"
        );
        assert_eq!(
            PrettyConfig::render_value(&Value::Array(
                Tag::Default,
                vec![Value::String(Tag::Default, "test".into())]
            )),
            "[test]"
        );
    }
    #[test]
    #[should_panic(expected = "dicts are not currently supported")]
    fn config_types_panic() {
        // This should panic, as we don't support dicts yet.
        let _ = PrettyConfig::render_value(&Value::Dict(Tag::Default, Map::new()));
    }
}
