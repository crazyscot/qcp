// (c) 2024 Ross Younger

//! Protocol feature compatibility definitions

use heck::ToUpperCamelCase;

use super::control::Compatibility;

// This macro exists to make it ergonomic to add more features.
// See the feature definition list below.
macro_rules! def_enum {
    (
        $(#[$attr:meta])*
        $vis:vis $name:ident => $ty:ty {
            $( $(#[$v_attr:meta])* $variant:ident => $val:expr),+
            $(,)?
        }
    ) => {
        $(#[$attr])*
        #[derive(PartialEq, Debug, Copy, Clone)]
        #[non_exhaustive]
        $vis struct $name($ty, &'static str);

        impl $name {
            $(
                $(#[$v_attr])*
                $vis const $variant: Self = Self($val, stringify!($variant));
            )+

            const VARIANTS: &'static [Self] = &[$(Self::$variant),+];
        }
    };
}

// This macro invocation generates the feature definition list (the Feature struct):

def_enum!(
    /// A utility mapping features by their symbolic name to their [`Compatibility`] level.
    ///
    /// This structure acts like an enum, but has extra crunchy flavour.
    ///
    /// ```
    /// use qcp::protocol::{control::Compatibility, compat::Feature};
    /// assert_eq!(Feature::BASIC_PROTOCOL.level(), Compatibility::Level(1));
    /// assert_eq!(Feature::BASIC_PROTOCOL.name(), "BASIC_PROTOCOL");
    /// ```

    pub Feature => Compatibility {
        /// The original base protocol introduced in qcp v0.3.0
        BASIC_PROTOCOL => Compatibility::Level(1),
        /// Support for the `NewReno` congestion control algorithm
        NEW_RENO => Compatibility::Level(2),
    }
);

impl Feature {
    /// Returns the compatibility level for a feature
    #[must_use]
    pub const fn level(self) -> Compatibility {
        self.0
    }

    /// Returns the symbolic name for a feature, in screaming snake case.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.1
    }

    /// The list of all known features.
    #[must_use]
    pub const fn variants() -> &'static [Self] {
        Self::VARIANTS
    }
}

impl Compatibility {
    #[must_use]
    /// Does this level support that feature?
    pub fn supports(self, feature: Feature) -> bool {
        match self {
            Compatibility::Unknown => false,
            Compatibility::Newer => true,
            Compatibility::Level(l) => l >= feature.level().into(),
        }
    }
}

// Pretty print support //////////////////////////////////////////////////////////////////////////////

#[derive(tabled::Tabled)]
struct TableRow {
    #[tabled(rename = "Feature")]
    name: String,
    compat: u16,
}

impl From<&Feature> for TableRow {
    fn from(f: &Feature) -> Self {
        Self {
            name: f.name().to_upper_camel_case(),
            compat: f.level().into(),
        }
    }
}

pub(crate) fn pretty_list() -> tabled::Table {
    let data = Feature::variants().iter().map(TableRow::from);
    tabled::Table::new(data)
}

#[cfg(test)]
mod test {
    use crate::protocol::control::Compatibility;

    use super::Feature;
    use heck::ToUpperCamelCase as _;

    #[test]
    fn list() {
        for it in Feature::variants() {
            eprintln!(
                "{} -> {} ({})",
                it.name().to_upper_camel_case(),
                it.level(),
                u16::from(it.level())
            );
        }
    }

    #[test]
    fn pretty() {
        let tbl = super::pretty_list();
        assert!(tbl.to_string().contains("BasicProtocol"));
    }

    #[test]
    fn supports() {
        assert!(Compatibility::Level(1).supports(Feature::BASIC_PROTOCOL));
        assert!(Compatibility::Newer.supports(Feature::BASIC_PROTOCOL));
        assert!(!Compatibility::Unknown.supports(Feature::BASIC_PROTOCOL));

        assert!(!Compatibility::Level(1).supports(Feature::NEW_RENO));
        assert!(Compatibility::Level(2).supports(Feature::NEW_RENO));
    }
}
