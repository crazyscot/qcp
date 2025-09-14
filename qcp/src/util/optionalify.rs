//! Macro to clone a structure for use with configuration data
// (c) 2024 Ross Younger

#![allow(meta_variable_misuse)] // false positives in these macro definitions

use derive_deftly::define_derive_deftly;

define_derive_deftly! {
    /// Clones a structure for use with CLI ([`clap`](https://docs.rs/clap/)) and options managers ([`figment`](https://docs.rs/figment/)).
    ///
    /// The expected use case is for configuration structs.
    /// The idea is that you define your struct how you want, then automatically generate a variety of the struct
    /// to make life easier. The variant:
    /// * doesn't require the user to enter all parameters (everything is an `Option`)
    /// * implements the [`figment::Provider`](https://docs.rs/figment/latest/figment/trait.Provider.html) helper trait
    /// which makes it easy to extract only the parameters the user entered.
    ///
    /// Of course, you would not set `default_value` attributes, because you would normally register
    /// the defaults with the configuration system at a different place (e.g. by implementing the [`Default`][std::default::Default] trait).
    ///
    /// The new struct:
    /// * is named `{OriginalName}_Optional`
    /// * has the same fields as the original, with all their attributes, but with their types wrapped
    /// in [`std::option::Option`]. (Yes, even any that were already `Option<...>`.)
    /// * contains exactly the same attributes as the original, plus `#[derive(Default)]` *(see note)*.
    /// * has the same visibility as the original, though you can override this with `#[deftly(visibility = ...)]`
    ///
    /// **Note:**
    /// If you already derived Default for the original struct, add the
    /// attribute ```#[deftly(already_has_default)]```.
    /// This tells the macro to *not* add ```#[derive(Default)]```, avoiding a compile error.
    ///
    /// <div class="warning">
    /// CAUTION: Attribute ordering is crucial. All attributes to be cloned to the new struct
    /// must appear <i>after</i> deriving `Optionalify`. It might look something like this:
    /// </div>
    ///
    /// ### Example
    ///
    /// ```
    /// use derive_deftly::Deftly;
    /// use qcp::derive_deftly_template_Optionalify;
    /// #[derive(Deftly)]
    /// #[derive_deftly(Optionalify)]
    /// #[derive(Debug, Clone /*, WhateverElseYouNeed...*/)]
    /// struct MyStruct {
    ///     /* ... */
    /// }
    /// ```
    ///
    /// ### Overriding serde attributes
    ///
    /// With some types, it is necessary to override the serde deserialization attributes
    /// on certain fields of the derived struct. To do this, use a `#[deftly(serde = "...")]` attribute.
    ///
    /// The contents of the `serde` meta-attribute replace any `serde` attribute on the field.
    /// Note that it is necessary to escape any quotes in the replacement attribute.
    ///
    /// For example:
    ///
    /// ```ignore
    /// struct MyStruct {
    ///     // ... fields ...
    ///
    ///     #[serde(deserialize_with = "foo::bar")]
    ///     #[deftly(serde = "deserialize_with = \"foo::qux\"")]
    ///     pub my_field: MyWeirdType,
    ///
    ///     // ... more fields ...
    /// }
    /// ```
    ///
    /// This doesn't work for `serialize_with` if the type also needs to be serialized differently.
    /// (case in point: `CongestionController`)
    /// For this situation, we support `#[deftly(serialize_with = "path::to::some::function")]`.
    ///
    /// ### Troubleshooting
    ///
    /// As with other template macros created with [`derive-deftly`](https://docs.rs/derive-deftly/), if you need
    /// to see what's going on you can use `#[derive_deftly(Optionalify[dbg])]` instead of `#[derive_deftly(Optionalify)]`
    /// to see the expanded output at compile time. Or, if you prefer, `cargo expand`.
    export Optionalify for struct, expect items:
    ${define OPTIONAL_TYPE ${paste $tdeftype _Optional}}

    /// Auto-derived struct variant with fields wrapped as `Option<...>`
    ///
    #[allow(non_camel_case_types)]
    ${tattrs}
    ${if not(tmeta(already_has_default)){
        #[derive(Default)]
    }}
    ${if tmeta(visibility) {
        ${tmeta(visibility) as token_stream}
    } else {
        ${tvis}
    }}
    struct $OPTIONAL_TYPE {
        $(
            // The 'serde' meta-attribute is an instruction to remove any serde attribute
            // on the field, and use the given string instead.
            ${if fmeta(serde) {
                ${fattrs ! serde, deftly}
                #[serde(${fmeta(serde) as token_stream})]
            } else {
                ${fattrs}
            }}
            ${fvis} $fname: Option<$ftype>,
            // Yes, if $ftype is Option<T>, the derived struct ends up with Option<Option<T>>. That's OK.
        )
    }

    impl figment::Provider for $OPTIONAL_TYPE {
        fn metadata(&self) -> figment::Metadata {
            figment::Metadata::named("command-line").interpolater(|_profile, path| {
                use heck::ToKebabCase;
                let key = path.last().map_or("<unknown>".to_string(), |s| s.to_kebab_case());
                format!("--{key}")
            })
        }

        fn data(&self) -> Result<figment::value::Map<figment::Profile, figment::value::Dict>, figment::Error> {
            use figment::{Profile, value::{Dict, Map, Value}};
            let mut dict = Dict::new();

            // Curveball: For some types, we want to serialize them differently
            // in different contexts. So we have the `serialize_with` meta attribute,
            // which works like `#[serde(serialize_with = ...)]`.
            $(
                if let Some(inner) = &self.${fname} {
                    let value =
                        ${if fmeta(serialize_with) {
                            // If there is a special serializer, use that
                            ${fmeta(serialize_with) as token_stream}(inner)?.into()
                        } else {
                            // Else, standard serialization.
                            Value::serialize(inner)?
                        }};
                    let _ = dict.insert(stringify!($fname).to_string(), value);
                }
            )

            let mut profile_map = Map::new();
            let _ = profile_map.insert(Profile::Global, dict);

            Ok(profile_map)
        }
    }
    impl From<&$tdeftype> for $OPTIONAL_TYPE {
        fn from(value: &$tdeftype) -> Self {
            Self {
                $(
                    $fname: Some(value.$fname.clone()),
                )
            }
        }
    }
}

#[allow(clippy::module_name_repetitions)]
pub use derive_deftly_template_Optionalify;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use super::derive_deftly_template_Optionalify;
    use derive_deftly::Deftly;
    use figment::{Figment, providers::Serialized};
    use pretty_assertions::assert_eq;

    #[derive(Deftly)]
    #[derive_deftly(Optionalify)]
    #[deftly(already_has_default)]
    #[derive(PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
    struct Foo {
        bar: i32,
        baz: String,
        wibble: Option<String>,
        q: Option<i32>,
    }

    #[test]
    fn optionality() {
        let mut entered = Foo_Optional::default();
        assert!(entered.bar.is_none());
        entered.bar = Some(999);
        entered.wibble = Some(Some("hi".to_string()));
        entered.q = Some(Some(123));

        //println!("simulated cli: {entered:?}");
        let f = Figment::new()
            .merge(Serialized::defaults(Foo::default()))
            .merge(entered);
        let working: Foo = f.extract().expect("extract failed");

        let expected = Foo {
            bar: 999,
            baz: String::new(), // default
            wibble: Some("hi".into()),
            q: Some(123),
        };
        assert_eq!(expected, working);
    }
}
