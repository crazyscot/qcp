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
    sync::LazyLock,
};
use struct_field_names_as_array::FieldNamesAsSlice;
use tabled::{
    Table, Tabled,
    settings::{Color, Theme, object::Rows, style::Style},
};

static TABLE_STYLE: LazyLock<Theme> = LazyLock::new(|| {
    if cfg!(windows) {
        Style::psql().into()
    } else {
        Style::sharp().into()
    }
});

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
            Value::String(_tag, s) => s.to_string(),
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
                    todo!("unhandled Num case");
                }
            }
            Value::Empty(_tag, _) => "<empty>".into(),
            // we don't currently support dict types
            Value::Dict(_tag, _dict) => todo!(),
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
        let _ = writable.with(TABLE_STYLE.clone());
        if use_colours() {
            let _ = writable.modify(Rows::single(1), host_colour);
        }
        write!(f, "{writable}")
    }
}
