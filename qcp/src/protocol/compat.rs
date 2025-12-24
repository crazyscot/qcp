// (c) 2024 Ross Younger

//! Protocol feature compatibility definitions

use heck::ToUpperCamelCase;
use strum::VariantArray as _;

use super::control::Compatibility;

// This macro exists to make it ergonomic to add more features.
// See the feature definition list below.
macro_rules! def_enum {
    (
        $(#[$attr:meta])*
        $vis:vis $name:ident => $ty:ty {
            $( $(#[$v_attr:meta])* $variant:ident => $val:expr => $comment:literal),+
            $(,)?
        }
    ) => {
        $(#[$attr])*
        #[derive(PartialEq, Debug, Copy, Clone)]
        #[non_exhaustive]
        $vis struct $name($ty, &'static str, &'static str);

        impl $name {
            $(
                $(#[$v_attr])*
                #[doc = $comment]
                $vis const $variant: Self = Self($val, stringify!($variant), $comment);
            )+
        }
        impl strum::VariantArray for $name {
            const VARIANTS: &'static [Self] = &[$(Self::$variant),+];
        }
        /* easy enough if we need this:
        impl strum::VariantNames for $name {
            const VARIANTS: &'static [&'static str] = &[$(stringify!($variant)),+];
        }
        */
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
        // Syntax: SYMBOL => LEVEL => DOC COMMENT
        // N.B. doc comments are made available at runtime, so have to go through the macro

        BASIC_PROTOCOL => Compatibility::Level(1) => "The original base protocol introduced in qcp v0.3.0",
        NEW_RENO => Compatibility::Level(2) => "Support for the `NewReno` congestion control algorithm",
        PRESERVE => Compatibility::Level(2) => "Support for preserving file metadata",
        GET2_PUT2 => Compatibility::Level(2) => "Get2 and Put2 commands with extensible options.\n`FileHeaderV2` and `FileTrailerV2` structures with extensible metadata.",
        CMSG_SMSG_2 => Compatibility::Level(3) => "Version 2 of `ClientMessage` and `ServerMessage` with extensible attributes.\n`CredentialsType` enum.",
        MKDIR_SETMETA => Compatibility::Level(4) => "CreateDirectory and SetMetadata commands",
    }
    // Note: When adding a new compatibility level, don't forget to update OUR_COMPATIBILITY_LEVEL.
);

impl Feature {
    /// Returns the compatibility level for a feature
    #[must_use]
    pub const fn level(self) -> Compatibility {
        self.0
    }

    /// Returns the symbolic name for a feature, in screaming snake case.
    ///
    /// ```
    /// use qcp::protocol::{control::Compatibility, compat::Feature};
    /// assert_eq!(Feature::NEW_RENO.name(), "NEW_RENO");
    /// ```
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.1
    }

    /// Returns the doc comment for a feature
    #[must_use]
    pub const fn comment(&self) -> &'static str {
        self.2
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
    #[tabled(rename = "Level")]
    compat: u16,
    #[tabled(rename = "Notes")]
    notes: String,
}

impl From<&Feature> for TableRow {
    fn from(f: &Feature) -> Self {
        Self {
            name: f.name().to_upper_camel_case(),
            compat: f.level().into(),
            notes: f.comment().into(),
        }
    }
}

pub(crate) fn pretty_list() -> tabled::Table {
    let data = Feature::VARIANTS.iter().map(TableRow::from);
    tabled::Table::new(data)
}

#[cfg(test)]
mod test {
    use crate::protocol::control::Compatibility;
    use strum::VariantArray as _;

    use super::Feature;
    use heck::ToUpperCamelCase as _;

    #[test]
    fn list() {
        for it in Feature::VARIANTS {
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

        assert!(!Compatibility::Level(2).supports(Feature::CMSG_SMSG_2));
        assert!(Compatibility::Level(3).supports(Feature::CMSG_SMSG_2));
    }
}
