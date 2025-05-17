//! Macro to clone a structure for use with configuration data
// (c) 2024 Ross Younger

#![allow(meta_variable_misuse)] // false positives in these macro definitions

use derive_deftly::define_derive_deftly;
use figment::value::{Dict, Value};

/// Helper function for [`figment::Provider`](https://docs.rs/figment/latest/figment/trait.Provider.html) implementation
///
/// If the given `arg` is not None, inserts it into `dict` with key `arg_name`.
///
/// We cannot avoid the large-Err issue here as it is forced by implementing `figment::Provider`, hence `#[allow(clippy::result_large_err)]`.
#[allow(clippy::result_large_err)]
pub fn insert_if_some<T>(
    dict: &mut Dict,
    arg_name: &str,
    arg: Option<T>,
) -> Result<(), figment::Error>
where
    T: serde::Serialize,
{
    if let Some(a) = arg {
        let _ = dict.insert(arg_name.to_string(), Value::serialize(a)?);
    }
    Ok(())
}

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
    /// As with other template macros created with [`derive-deftly`](https://docs.rs/derive-deftly/), if you need
    /// to see what's going on you can use `#[derive_deftly(Optionalify[dbg])]` instead of `#[derive_deftly(Optionalify)]`
    /// to see the expanded output at compile time.
    export Optionalify for struct, expect items:
    ${define OPTIONAL_TYPE ${paste $tdeftype _Optional}}

    /// Auto-derived struct variant
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
            ${fattrs}
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
            use $crate::util::insert_if_some;
            use figment::{Profile, value::{Dict, Map}};
            let mut dict = Dict::new();

            $(
                insert_if_some(&mut dict, stringify!($fname), self.${fname}.clone())?;
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
