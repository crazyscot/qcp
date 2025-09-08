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

    /// Create a new [`TaggedData`] instance with the given variant and unsigned data.
    ///
    /// This is an ergonomic version of [`DataTag::with_variant`].
    fn with_unsigned<V: Into<u64>>(self, data: V) -> TaggedData<Self>
    where
        Self: Sized,
    {
        TaggedData::new(self, Variant::unsigned(data.into()))
    }

    /// Create a new [`TaggedData`] instance with the given variant and signed data.
    ///
    /// This is an ergonomic version of [`DataTag::with_variant`].
    fn with_signed<V: Into<i64>>(self, data: V) -> TaggedData<Self>
    where
        Self: Sized,
    {
        TaggedData::new(self, Variant::signed(data.into()))
    }

    /// Create a new [`TaggedData`] instance with the given variant and bytes data.
    ///
    /// This is an ergonomic version of [`DataTag::with_variant`].
    fn with_bytes<B: AsRef<[u8]>>(self, data: B) -> TaggedData<Self>
    where
        Self: Sized,
    {
        TaggedData::new(self, Variant::Bytes(data.as_ref().to_vec()))
    }

    /// Create a new [`TaggedData`] instance with the given variant and string data.
    ///
    /// This is an ergonomic version of [`DataTag::with_variant`].
    fn with_str<S: AsRef<str>>(self, data: S) -> TaggedData<Self>
    where
        Self: Sized,
    {
        TaggedData::new(self, Variant::String(data.as_ref().to_owned()))
    }

    /// Renders [`Variant`] data belonging to this tag.
    ///
    /// This is a helper for `Display` and `Debug` on [`TaggedData`].
    ///
    /// The default implementation looks up the symbolic tag, then calls [`DataTag::debug_data`] for that tag.
    /// If the value was not recognised as a tag, we call Debug on the variant data.
    /// This method may be overridden to handle that case differently.
    #[must_use]
    fn debug_data_for_tag(tag: u64, data: &Variant) -> String {
        Self::try_from(tag).map_or_else(|_| format!("{data:?}"), |tag| tag.debug_data(data))
    }
    /// Renders [`Variant`] data for tag-specific debug.
    ///
    /// This is a helper for `Display` and `Debug` on [`TaggedData`].
    ///
    /// The default implementation calls directly to Debug, but may be overridden for any special output (e.g. octal) that makes sense for the enum.
    #[must_use]
    fn debug_data(&self, data: &Variant) -> String {
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
#[derive(
    Serialize, Deserialize, PartialEq, Clone, Default, derive_more::Debug, derive_more::Display,
)]
#[display("({}, {})", self.tag_str(), E::debug_data_for_tag(tag.0, data))]
pub struct TaggedData<E: DataTag> {
    /// Option tag
    #[debug("{}::{}", last_component(std::any::type_name::<E>()),
        E::try_from(tag.0).map_or_else(|_| format!("UNKNOWN_{}", tag.0), |f| f.to_string()))]
    tag: Uint,
    /// Option data
    #[debug("{}", E::debug_data_for_tag(tag.0, data))]
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

    /// String representation of the option tag
    #[must_use]
    pub fn tag_str(&self) -> String {
        E::try_from(self.tag.0)
            .map_or_else(|_| format!("UNKNOWN_{}", self.tag.0), |f| f.to_string())
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

/// Helper trait for finding tagged data in collections
pub trait FindTag<E>: std::iter::IntoIterator
where
    E: DataTag,
    u64: From<E>,
{
    /// Looks up the given tag in this collection.
    /// Returns the associated variant data, if one was found.
    fn find_tag(&self, tag: E) -> Option<&Variant>;
}

impl<E> FindTag<E> for Vec<TaggedData<E>>
where
    E: DataTag,
    u64: From<E>,
{
    fn find_tag(&self, tag: E) -> Option<&Variant> {
        let wanted = u64::from(tag);
        self.iter()
            .find(|item| item.tag_raw() == wanted)
            .map(|item| &item.data)
    }
}

/// Helper function for implementing Display on `Vec<TaggedData<E>>`
pub(crate) fn display_vec_td<E: DataTag>(v: &Vec<TaggedData<E>>) -> String {
    if v.is_empty() {
        return "[]".into();
    }
    let mut s = String::new();
    s.push('[');
    let mut first = true;
    for it in v {
        if !first {
            s.push_str(", ");
        }
        first = false;
        s.push_str(&it.tag_str());
        s.push(':');
        s.push_str(&E::debug_data_for_tag(it.tag_raw(), &it.data));
    }
    s.push(']');
    s
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
        let tagged = TestTag::First.with_signed(42);
        let s = format!("{tagged:?}");
        assert_str_eq!(
            s,
            "TaggedData { tag: TestTag::First, data: Signed(42), .. }"
        );
        assert_eq!(tagged.tag(), Some(TestTag::First));
        assert_eq!(tagged.tag_raw(), 1);

        let s = format!("{tagged}");
        assert_str_eq!(s, "(First, Signed(42))");
        assert_eq!(tagged.tag(), Some(TestTag::First));
        assert_eq!(tagged.tag_raw(), 1);

        let tagged: TaggedData<TestTag> = TestTag::Second.into();
        let s = format!("{tagged:?}");
        assert_str_eq!(s, "TaggedData { tag: TestTag::Second, data: Empty, .. }");
        assert_eq!(tagged.tag(), Some(TestTag::Second));
        assert_eq!(tagged.tag_raw(), 2);

        let tagged: TaggedData<TestTag> = TestTag::Second.into();
        let s = format!("{tagged}");
        assert_str_eq!(s, "(Second, Empty)");
        assert_eq!(tagged.tag(), Some(TestTag::Second));
        assert_eq!(tagged.tag_raw(), 2);
    }

    #[derive(strum_macros::Display, Debug, IntEnum, PartialEq)]
    #[repr(u64)]
    enum TestTagCustomDebug {
        Weasels = 1,
        Wombats = 2,
    }
    impl DataTag for TestTagCustomDebug {
        fn debug_data(&self, data: &Variant) -> String {
            match self {
                TestTagCustomDebug::Weasels => {
                    format!("{} weasels", data.coerce_unsigned())
                }
                TestTagCustomDebug::Wombats => format!("{data:?}"),
            }
        }
    }
    #[test]
    fn tagged_data_custom_debug() {
        let tagged = TestTagCustomDebug::Weasels.with_signed(42);
        let s = format!("{tagged:?}");
        assert_str_eq!(
            s,
            "TaggedData { tag: TestTagCustomDebug::Weasels, data: 42 weasels, .. }"
        );

        let s = format!("{tagged}");
        assert_str_eq!(s, "(Weasels, 42 weasels)");
    }
}
