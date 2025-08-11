// (c) 2025 Ross Younger

//! Data tagging for attributes in QCP protocol messages

use serde::{Deserialize, Serialize};
use serde_bare::Uint;
use std::marker::PhantomData;

use crate::protocol::Variant;

/// Marker trait for enums that can be used in [`TaggedData`].
///
/// These enums need:
/// * to be declared with `#[repr(u64)]`
/// * to implement [`std::fmt::Display`] (typically via [`strum_macros::Display`])
/// * to be convertible into a `u64` (typically by deriving [`int_enum::IntEnum`])
pub trait DataTag: Into<u64> + TryFrom<u64> + ToString {
    /// Create a new [`TaggedData`] instance with the given variant and data.
    ///
    /// If no data is required (the Variant is empty), you can also use `TaggedData::from(enum)`.
    fn with_variant(self, data: Variant) -> TaggedData<Self>
    where
        Self: Sized,
    {
        TaggedData::new(self, data)
    }

    /// Renders [`Variant`] data for tag-specific debug.
    ///
    /// The default implementation calls directly to Debug. See [`DataTag::debug_data_inner`], which may be overridden for any special output (e.g. octal) that makes sense for the enum.
    #[must_use]
    fn debug_data(value: u64, data: &Variant) -> String {
        Self::try_from(value).map_or_else(|_| format!("{data:?}"), |tag| tag.debug_data_inner(data))
    }
    /// Renders [`Variant`] data for tag-specific debug.
    ///
    /// The default implementation calls directly to Debug, but may be overridden for any special output (e.g. octal) that makes sense for the enum.
    #[must_use]
    fn debug_data_inner(&self, data: &Variant) -> String {
        format!("{data:?}")
    }
}

impl<E: DataTag> From<E> for TaggedData<E> {
    fn from(value: E) -> Self {
        TaggedData::new(value, Variant::Empty)
    }
}

#[allow(dead_code)] // false positive
fn last_component(tn: &'static str) -> &'static str {
    tn.rsplit("::").next().unwrap_or(tn)
}

/// A tagging enum with its attached data.
///
/// To make an enum capable of being used in this struct, implement the [`DataTag`] trait and declare `#[repr(u64)]`.
#[derive(Serialize, Deserialize, PartialEq, Clone, derive_more::Debug)]
pub struct TaggedData<E: DataTag> {
    /// Option tag
    #[debug("{}::{}", last_component(std::any::type_name::<E>()),
        E::try_from(tag.0).map_or_else(|_| format!("UNKNOWN_{}", tag.0), |f| f.to_string()))]
    tag: Uint,
    /// Option data
    #[debug("{}", E::debug_data(tag.0, data))]
    pub data: Variant,
    #[debug(ignore)]
    #[serde(skip)]
    phantom: PhantomData<E>,
}

impl<E: DataTag> TaggedData<E> {
    /// Standard constructor (but for better ergonomics, see [`DataTag::with_variant`])
    #[must_use]
    pub fn new(option: E, data: Variant) -> Self {
        Self {
            tag: Uint(option.into()),
            data,
            phantom: PhantomData,
        }
    }

    /// Accessor for the option tag.
    /// In the event that the tag is unknown to enum `E` (i.e. from a newer protocol version), returns `None`.
    #[must_use]
    pub fn tag(&self) -> Option<E>
    where
        E: TryFrom<u64>,
    {
        E::try_from(self.tag.0).ok()
    }

    /// Accessor for the option tag.
    #[must_use]
    pub fn tag_raw(&self) -> u64 {
        self.tag.0
    }

    #[cfg(test)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    /// Test constructor, allows struct to be constructed with an invalid tag
    pub(crate) fn new_raw(opt: u64) -> Self {
        Self {
            tag: Uint(opt),
            data: Variant::Empty,
            phantom: PhantomData,
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use int_enum::IntEnum;
    use pretty_assertions::{assert_eq, assert_str_eq};

    use crate::protocol::Variant;

    use super::{DataTag, TaggedData};

    #[derive(strum_macros::Display, Debug, IntEnum, PartialEq)]
    #[repr(u64)]
    enum TestTag {
        First = 1,
        Second = 2,
    }
    impl DataTag for TestTag {}

    #[test]
    fn tagged_data() {
        let tagged = TestTag::First.with_variant(Variant::signed(42));
        let s = format!("{tagged:?}");
        assert_str_eq!(
            s,
            "TaggedData { tag: TestTag::First, data: Signed(42), .. }"
        );
        assert_eq!(tagged.tag(), Some(TestTag::First));
        assert_eq!(tagged.tag_raw(), 1);

        let tagged: TaggedData<TestTag> = TestTag::Second.into();
        let s = format!("{tagged:?}");
        assert_str_eq!(s, "TaggedData { tag: TestTag::Second, data: Empty, .. }");
        assert_eq!(tagged.tag(), Some(TestTag::Second));
        assert_eq!(tagged.tag_raw(), 2);
    }
    // debug_data_inner; others
}
