// (c) 2025 Ross Younger
//! Variant object type

use std::{collections::BTreeMap, convert::identity, fmt::Write};

use dtype_variant::DType;
use paste::paste;
use serde::{Deserialize, Serialize};
use serde_bare::{Int, Uint};

/// A list ([`Vec<Variant>`]). This can itself be stored in a [`Variant`].
pub type VariantList = Vec<Variant>;
/// A map ([`BTreeMap<String, Variant>`]). This can itself be stored in a [`Variant`].
pub type VariantMap = BTreeMap<String, Variant>;

/// A serializable Variant data type (inspired by Qt et al) for passing around arbitrary data.
///
/// Goals:
/// - Serializable via [`serde_bare`]
/// - Support a variety of data types, including integers, strings, lists, and maps.
/// - Enable a forwards-compatible wire-protocol design.
/// - Good ergonomics.
///
/// <div class="warning">
/// The receiver is responsible for checking that the type of the variant data is appropriate for the operation.
/// </div>
///
/// This struct was introduced in qcp 0.5 with `VersionCompatibility=V2`.
///
/// # Example use
///
/// ```rust
/// use qcp::protocol::Variant;
///
/// // Variants can be constructed directly from simple types.
/// let var1 = Variant::from("Hello, World!");
/// let var2 = Variant::from(());
/// let var3 = Variant::from(true);
/// let mystring = String::from("Hello, World!");
/// let var4 = Variant::from(mystring); // consumes mystring
/// assert_eq!(var1, var4);
///
/// // You can test their types:
/// assert!(var1.is_string());
/// assert!(var2.is_empty());
/// assert!(var3.is_boolean());
///
/// // They convert to strings:
/// assert_eq!(var1.to_string(), r"Hello, World!");
/// assert_eq!(var2.to_string(), r"()");
/// assert_eq!(var3.to_string(), r"true");
/// // (Beware! `to_string()` and `into_string()` are different! One creates a new string
/// //  _representation_ of an existing Variant; the other consumes the Variant and returns
/// //  the String within.)
/// ```
///
/// ## Referencing and extraction
///
/// You can obtain a reference if you expect a certain inner type.
/// This borrows the variant without consuming it.
///
/// You can downcast to the inner type, consuming the `Variant`, or safely downcast in a way that the
/// returned `Error` contains the original `Variant`.
///
/// ```rust
/// use qcp::protocol::Variant;
/// let mut var3 = Variant::from(true);
/// let mut r = var3.as_bool_ref();
/// assert_eq!(r, Some(&true));
/// // Similarly for a mutable reference, if the underlying variable is mutable:
/// let r = var3.as_bool_mut().unwrap();
/// *r = false;
/// assert_eq!(var3.to_string(), r"false");
///
/// // Extract the inner data, consuming the variant:
/// let b = var3.into_bool();
/// assert_eq!(b, Some(false)); // because we modified var3 a few lines ago
///
/// // Extraction if you need to be able to get the variant back if it's not the expected type:
/// let var3 = Variant::from(true);
/// let s = var3.try_into_list();
/// let s2 = s.unwrap_err().0.try_into_bool();
/// assert_eq!(s2.unwrap(), true);
/// ```
///
/// ## Integer types
///
/// Integers are stored as `i64` ([`Variant::Signed`]) or `u64` ([`Variant::Unsigned`]).
///
/// As it's common to want to create a Variant directly from an integer literal,
/// we provide convenient constructors for that. As literals can be ambiguous, these specify the type explicitly.
/// They also have the benefit that they upcast automatically where safe.
///
/// ```rust
/// use qcp::protocol::Variant;
///
/// let var4 = Variant::signed(42);
/// // let var5 = Variant::unsigned(42); // this doesn't compile! Numeric literals are u32.
/// let var5 = Variant::unsigned(42u8); // This is one way to construct from a literal
///
/// // Or you can type-coerce, if you are willing to accept the consequences.
/// let var6 = Variant::unsigned_coerce(42);
/// let var7 = Variant::signed_coerce(42);
///
/// // Equality works only where the contained type matches.
/// assert_eq!(var4, var7);
/// assert_eq!(var5, var6);
/// assert_ne!(var4, var6); // Signed != Unsigned
/// ```
///
/// ## Type coercion
///
/// Limited coercion is supported. See [`coerce_bool`](#method.coerce_bool),
/// [`coerce_signed`](#method.coerce_signed), and [`coerce_unsigned`](#method.coerce_unsigned).
///
/// ## Ergonomic construction and extraction
///
/// See:
/// * `String`:
///   * [`From<&str>`](#impl-From<%26str>-for-Variant)
///   * [`From<String>`](#impl-From<String>-for-Variant)
///   * [`as_string`](#method.as_string)
///   * [`as_str`](#method.as_str)
///   * [`as_str_mut`](#method.as_str_mut)
/// * `Bytes`:
///   * [`From<&[u8]>`](#impl-From<%26[u8]>-for-Variant)
///   * [`From<Vec<u8>>`](#impl-From<Vec<u8>>-for-Variant)
///   * [`as_bytes`](#method.as_bytes)
///   * [`as_slice_bytes`](#method.as_slice_bytes)
///   * [`as_slice_bytes_mut`](#method.as_slice_bytes_mut)
/// * `List`:
///   * [`From<VariantList>`](#impl-From<Vec<Variant>>-for-Variant)
///   * [`From<&[Variant]>`](#impl-From<&[Variant]>-for-Variant)
///   * [`as_list`](#method.as_list)
///   * [`as_slice_variant`](#method.as_slice_variant)
///   * [`as_slice_variant_mut`](#method.as_slice_variant_mut)
/// * `Map`:
///   * [`From<VariantMap>`](#impl-From<BTreeMap<String,+Variant>>-for-Variant)
///   * [`as_map`](#method.as_map)
#[derive(
    DType,
    Serialize,
    Deserialize,
    PartialEq,
    Clone,
    derive_more::Debug,
    derive_more::Display,
    strum_macros::EnumIs,
)]
pub enum Variant {
    /// The unit type `()`, used for empty or no-value cases.
    #[display("()")]
    Empty,
    /// True or false
    Boolean(bool),
    /// Signed 64-bit integer
    #[display("{0}", _0.0)]
    #[debug("Signed({0})", _0.0)]
    Signed(Int),
    /// Unsigned 64-bit integer
    #[display("{0}", _0.0)]
    #[debug("Unsigned({0})", _0.0)]
    Unsigned(Uint),
    /// A UTF-8 encoded string
    String(String),
    /// A plain byte array
    #[display("0x{}", hex::encode(_0))]
    Bytes(Vec<u8>),
    /// A [`Vec<Variant>`], which can be used to represent lists or arrays.
    #[display("{}", VariantListWrap(_0))]
    List(VariantList),
    /// A [`BTreeMap<String, Variant>`], which can be used to represent dictionaries or objects.
    #[display("{}", VariantMapWrap(_0))]
    Map(VariantMap),
}

struct VariantListWrap<'a>(&'a VariantList);

fn fmt_container(f: &mut std::fmt::Formatter<'_>, it: &Variant) -> std::fmt::Result {
    match it {
        Variant::String(s) => f.write_fmt(format_args!(r#""{s}""#)),
        _ => f.write_fmt(format_args!("{it}")),
    }
}

impl std::fmt::Display for VariantListWrap<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_char('[')?;
        let mut first = true;
        for it in self.0 {
            if !first {
                f.write_str(", ")?;
            }
            fmt_container(f, it)?;
            first = false;
        }
        f.write_char(']')
    }
}

struct VariantMapWrap<'a>(&'a VariantMap);

impl std::fmt::Display for VariantMapWrap<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_char('{')?;
        let mut first = true;
        for (k, v) in self.0 {
            if !first {
                f.write_str(", ")?;
            }
            f.write_fmt(format_args!(r#""{k}": "#))?;
            fmt_container(f, v)?;
            first = false;
        }
        f.write_char('}')
    }
}

// Integer ergonomic constructors ==========================================
impl Variant {
    /// Ergonomic constructor for signed integers
    #[must_use]
    pub fn signed<T: Into<i64>>(i: T) -> Self {
        Variant::Signed(Int(i.into()))
    }
    /// Ergonomic constructor for unsigned integers
    #[must_use]
    pub fn unsigned<T: Into<u64>>(u: T) -> Self {
        Variant::Unsigned(Uint(u.into()))
    }

    /// Ergonomic type-coercing constructor for signed integers
    ///
    /// <div class="warning">
    /// This constructor silently casts its input to `i64`.
    /// There is no overflow or loss-of-sign detection.
    /// </div>
    pub fn signed_coerce<T>(u: T) -> Self
    where
        T: num_traits::cast::AsPrimitive<i64>,
    {
        Variant::Signed(Int(u.as_()))
    }

    /// Ergonomic type-coercing constructor for unsigned integers
    ///
    /// <div class="warning">
    /// This constructor silently casts its input to `u64`.
    /// There is no overflow or loss-of-sign detection.
    /// </div>
    pub fn unsigned_coerce<T>(u: T) -> Self
    where
        T: num_traits::cast::AsPrimitive<u64>,
    {
        Variant::Unsigned(Uint(u.as_()))
    }
}

// Direct constructors ======================================================

// Integer
macro_rules! from_types {
    ($var:ident, $cls:ident, $($t:ty),+) => {$(
        impl From<$t> for Variant {
            fn from(value: $t) -> Self {
                Variant::$var($cls(value.into()))
            }
        }
    )+}
}

from_types!(Unsigned, Uint, u64, u32, u16, u8);
from_types!(Signed, Int, i64, i32, i16, i8);

impl From<()> for Variant {
    fn from((): ()) -> Self {
        Variant::Empty
    }
}

// Special case constructors
impl From<&str> for Variant {
    fn from(value: &str) -> Self {
        Variant::String(value.to_string())
    }
}

impl<const N: usize> From<&[u8; N]> for Variant {
    fn from(value: &[u8; N]) -> Self {
        Variant::Bytes(value.to_vec())
    }
}

impl<const N: usize> From<&[Variant; N]> for Variant {
    fn from(value: &[Variant; N]) -> Self {
        Variant::List(value.to_vec())
    }
}

// Extraction and referencing =======================================================

macro_rules! as_variant_fn {
    ($fname:ident, $vartype:ident, $inner:ty, $map_to:expr, $map_as:expr, $map_mut:expr) => {
        paste! {
        impl Variant {
            /// Extract the inner data, if the variant is of that type.
            /// (If not, the variant is destroyed!)
            #[must_use]
            pub fn [<into_ $fname>](self) -> Option<$inner> {
                self.downcast::<[<$vartype Variant>]>().map($map_to)
            }
            /// Extract the inner data, if the variant is of that type.
            /// If not, returns a [`VariantConversionFailed`] which contains the variant.
            pub fn [<try_into_ $fname>](self) -> Result<$inner, VariantConversionFailed> {
                if let Variant::$vartype(d) = self {
                    return Ok($map_to(d));
                }
                Err(VariantConversionFailed(self))
            }
            /// Obtain a reference to the inner data, if the variant is of that type.
            #[must_use]
            pub fn [<as_ $fname _ref>](&self) -> Option<&$inner> {
                self.downcast_ref::<[<$vartype Variant>]>().map($map_as)
            }
            /// Obtain a mutable reference to the inner data, if the variant is of that type.
            #[must_use]
            pub fn [<as_ $fname _mut>](&mut self) -> Option<&mut $inner> {
                self.downcast_mut::<[<$vartype Variant>]>().map($map_mut)
            }

        }
        }
    };
}
macro_rules! as_variant_id {
    ($fname:ident, $vartype:ident, $inner:ty) => {
        as_variant_fn!($fname, $vartype, $inner, identity, identity, identity);
    };
}

// Direct types with no wrapping inner type
as_variant_id!(bool, Boolean, bool);
as_variant_id!(bytes, Bytes, Vec<u8>);
as_variant_id!(string, String, String);
as_variant_id!(list, List, VariantList);
as_variant_id!(map, Map, VariantMap);

// Integer types are special due to the Int/Uint BARE wrapping
as_variant_fn!(unsigned, Unsigned, u64, |u: Uint| u.0, |u| &u.0, |u| &mut u
    .0);
as_variant_fn!(signed, Signed, i64, |i: Int| i.0, |i| &i.0, |i| &mut i.0);

// Special case referencing (analogous to From<&str>, etc.)
impl Variant {
    /// Borrow a reference to the inner `&str`, if the variant is a `Variant::String`.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        self.downcast_ref::<StringVariant>()
            .map(std::convert::AsRef::as_ref)
    }
    /// Borrow a mutable reference to the inner `&str`, if the variant is a `Variant::String`.
    #[must_use]
    pub fn as_str_mut(&mut self) -> Option<&mut str> {
        self.downcast_mut::<StringVariant>()
            .map(std::convert::AsMut::as_mut)
    }

    /// Borrow a reference to the inner byte slice, if the variant is a `Variant::Bytes`.
    #[must_use]
    pub fn as_slice_bytes(&self) -> Option<&[u8]> {
        self.downcast_ref::<BytesVariant>()
            .map(std::convert::AsRef::as_ref)
    }

    /// Borrow a mutable reference to the inner byte slice, if the variant is a `Variant::Bytes`.
    #[must_use]
    pub fn as_slice_bytes_mut(&mut self) -> Option<&mut [u8]> {
        self.downcast_mut::<BytesVariant>()
            .map(std::convert::AsMut::as_mut)
    }

    /// Borrow a reference to the inner variant slice, if the variant is a `Variant::List`.
    #[must_use]
    pub fn as_slice_variant(&self) -> Option<&[Variant]> {
        self.downcast_ref::<ListVariant>()
            .map(std::convert::AsRef::as_ref)
    }

    /// Borrow a mutable reference to the inner variant slice, if the variant is a `Variant::List`.
    #[must_use]
    pub fn as_slice_variant_mut(&mut self) -> Option<&mut [Variant]> {
        self.downcast_mut::<ListVariant>()
            .map(std::convert::AsMut::as_mut)
    }
}

/// Error type for `try_into_` conversions.
///
/// ```rust
/// use qcp::protocol::{Variant, VariantConversionFailed};
///
/// let var = Variant::from(true);
/// let res = var.try_into_string(); // this consumes `var`
/// if let Err(v) = res {
///     // do something useful with `v`, which is the original `Variant`
///     assert_eq!(v.0.as_bool_ref(), Some(&true));
/// }
/// ```
#[derive(thiserror::Error, Debug, derive_more::Display)]
#[display("VariantConversionFailed({_0})")]
pub struct VariantConversionFailed(pub Variant);

// Limited coercion ====================================================================

impl Variant {
    /// Coerces the variant into a boolean.
    ///
    /// Integers map to false if 0; otherwise 1.
    ///
    /// Strings, bytes, lists and maps are always false.
    #[must_use]
    pub fn coerce_bool(&self) -> bool {
        match self {
            Variant::Boolean(b) => *b,
            Variant::Unsigned(Uint(u)) => *u != 0,
            Variant::Signed(Int(i)) => *i != 0,
            _ => false,
        }
    }
    /// Coerces the variant into a signed 64-bit value.
    ///
    /// Integers are cast; booleans map true to 1, false to 0; strings, bytes, lists and maps are always 0.
    #[must_use]
    pub fn coerce_signed(&self) -> i64 {
        use num_traits::AsPrimitive as _;
        match self {
            Variant::Signed(Int(i)) => *i,
            Variant::Unsigned(Uint(u)) => (*u).as_(),
            Variant::Boolean(b) => i64::from(*b),
            _ => 0,
        }
    }
    /// Coerces the variant into an unsigned 64-bit value.
    ///
    /// Integers are cast; booleans map true to 1, false to 0; strings, bytes, lists and maps are always 0.
    #[must_use]
    pub fn coerce_unsigned(&self) -> u64 {
        use num_traits::AsPrimitive as _;
        match self {
            Variant::Signed(Int(i)) => (*i).as_(),
            Variant::Unsigned(Uint(u)) => *u,
            Variant::Boolean(b) => u64::from(*b),
            _ => 0,
        }
    }

    // There is no coercion to string (use to_string!), bytes, list or map.
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use assertables::assert_matches;
    use pretty_assertions::assert_eq;
    use serde_bare::{Int, Uint};

    use crate::protocol::{VariantConversionFailed, common::ProtocolMessage};

    use super::{Variant, VariantMap};
    #[test]
    fn creation_and_stringify() {
        macro_rules! test_var {
            ($val:expr) => {
                let v = Variant::from($val);
                eprintln!("{v}");
            };

            ($val:expr, $expect:expr) => {
                let v = Variant::from($val);
                eprintln!("{v}");
                assert_eq!(v.to_string(), $expect);
                //eprintln!("{v:?}");
                //eprintln!("{v:#?}");
            };
        }
        test_var!((), "()");
        test_var!(true, "true");
        test_var!(false, "false");
        test_var!("hello".to_string(), "hello");
        test_var!("hello", "hello");
        test_var!(vec![0, 1, 2, 3, 4], "0x0001020304");
        test_var!(&[7, 6, 5, 4], "0x07060504");

        let list = &[
            Variant::from(true),
            Variant::unsigned(0u8),
            Variant::from("whee"),
        ];
        test_var!(list, r#"[true, 0, "whee"]"#);
        test_var!(list.to_vec(), r#"[true, 0, "whee"]"#);

        let mut map = VariantMap::new();
        let _ = map.insert("foo".into(), "bar".into());
        let _ = map.insert("baz".into(), Variant::signed(42));
        test_var!(map, r#"{"baz": 42, "foo": "bar"}"#);
    }

    #[test]
    fn construct_upcast_ints() {
        let v = Variant::signed(42i16);
        assert_matches!(v, Variant::Signed(Int(42)));
        let v = Variant::unsigned_coerce(42);
        assert_matches!(v, Variant::Unsigned(Uint(42)));
    }

    #[test]
    fn downcasting() {
        let mut v = Variant::from(false);
        let r = v.as_bool_ref();
        assert_matches!(r, Some(&false));
        let r = v.as_bool_mut().unwrap();
        *r = true;
        // Did it mutate?
        let r = v.into_bool().unwrap();
        assert!(r);

        let mut v = Variant::unsigned(42u8);
        let r = v.as_unsigned_mut().unwrap();
        *r = 1234;
        let r = v.as_unsigned_ref().unwrap();
        assert_eq!(*r, 1234);
        let r = v.into_unsigned();
        assert_matches!(r, Some(1234));

        let v = Variant::signed(-4);
        let r = v.into_signed();
        assert_matches!(r, Some(-4));

        // false converts to integer, so you can upcast in a slightly surprising way:
        let v = Variant::unsigned(false);
        let r = v.into_unsigned();
        assert_matches!(r, Some(0));
    }

    #[test]
    fn downcast_list() {
        let mut v = Variant::from(vec![
            Variant::from(true),
            Variant::signed(-4),
            Variant::from("hi"),
        ]);
        assert!(v.is_list());
        let r = v.as_list_mut().unwrap();
        assert!(r.len() == 3);
        assert!(r[0].as_bool_ref() == Some(&true));
    }

    #[test]
    fn downcast_map() {
        let mut map = VariantMap::new();
        let _ = map.insert("foo".into(), "bar".into());
        let _ = map.insert("baz".into(), Variant::signed(42));
        let mut v = Variant::from(map);

        let r = v.as_map_mut().unwrap();
        assert!(r.len() == 2);
        assert_eq!(
            r.get_mut("foo").and_then(|v| v.as_string_mut()),
            Some(&mut "bar".to_string())
        );
        assert_eq!(r.get("baz").and_then(Variant::as_signed_ref), Some(&42));
    }

    #[test]
    fn conversion_fail() {
        let var = Variant::from(1234);
        let res = var.try_into_bool();
        assert_matches!(
            res,
            Err(VariantConversionFailed(Variant::Signed(Int(1234))))
        );
    }

    #[test]
    fn ref_inner_str() {
        let mut var = Variant::from("hello");
        let r = var.as_str();
        assert_eq!(r, Some("hello"));
        let r = var.as_str_mut();
        if let Some(rr) = r {
            // There's not much you can safely do with an `&mut str`, but here's one to prove the point:
            rr.make_ascii_uppercase();
        }
        assert_eq!(var.as_str(), Some("HELLO"));
    }

    #[test]
    fn ref_inner_bytes() {
        let mut var = Variant::from(&[1, 2, 3, 4, 5]);
        let r = var.as_slice_bytes().unwrap();
        assert_eq!(r.len(), 5);
        assert_eq!(r[0], 1);

        let r = var.as_slice_bytes_mut().unwrap();
        r[0] = 42;

        assert_eq!(var.into_bytes(), Some(vec![42u8, 2, 3, 4, 5]));
    }

    #[test]
    fn ref_inner_variant_list() {
        let mut var = Variant::from(vec![
            Variant::from(true),
            Variant::signed(-4),
            Variant::from("hi"),
        ]);

        let r = var.as_slice_variant().unwrap();
        assert!(r.len() == 3);
        assert!(r[0].as_bool_ref() == Some(&true));

        let r = var.as_slice_variant_mut().unwrap();
        r[1] = Variant::from(false);

        assert_eq!(
            var.into_list(),
            Some(vec![
                Variant::from(true),
                Variant::from(false),
                Variant::from("hi"),
            ])
        );
    }

    impl ProtocolMessage for Variant {}

    fn test_encode(v: &Variant, expected: &[u8]) {
        let encoded = v.to_vec().unwrap();
        assert_eq!(encoded, expected, "failing case: {:?}", v);
        let decoded = Variant::from_slice(&encoded).unwrap();
        assert_eq!(*v, decoded, "failing case: {:?}", v);
    }

    #[test]
    fn ser_de_empty() {
        test_encode(&Variant::Empty, &[0u8]);
    }

    #[test]
    fn ser_de_bool() {
        test_encode(&Variant::from(true), &[1u8, 1]);
        test_encode(&Variant::from(false), &[1u8, 0]);
    }

    #[test]
    fn ser_de_int() {
        test_encode(&Variant::signed(42), &[2u8, 84]);
        test_encode(&Variant::signed(-2), &[2u8, 3]);
        test_encode(&Variant::signed(0), &[2u8, 0]);
        test_encode(&Variant::signed(1234), &[2u8, 164, 19]);
        test_encode(
            &Variant::signed(-9_223_372_036_854_775_807i64),
            &[2u8, 253, 255, 255, 255, 255, 255, 255, 255, 255, 1],
        );

        test_encode(&Variant::unsigned_coerce(42), &[3u8, 42]);
        test_encode(
            &Variant::unsigned(18_446_744_073_709_551_615u64),
            &[3u8, 255, 255, 255, 255, 255, 255, 255, 255, 255, 1],
        );
    }

    #[test]
    fn ser_de_str() {
        test_encode(&Variant::from("hello"), &[4u8, 5, 104, 101, 108, 108, 111]);
    }
    #[test]
    fn ser_de_bytes() {
        test_encode(&Variant::from(&[1, 2, 3, 4, 5]), &[5u8, 5, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn ser_de_list() {
        let list = vec![
            Variant::from(true),
            Variant::unsigned(0u8),
            Variant::from("whee"),
        ];
        test_encode(
            &Variant::from(list),
            &[6u8, 3, 1, 1, 3, 0, 4, 4, 119, 104, 101, 101],
        );
    }

    #[test]
    fn ser_de_map() {
        let map = {
            let mut m = VariantMap::new();
            let _ = m.insert("foo".into(), "bar".into());
            let _ = m.insert("baz".into(), Variant::signed(42));
            m
        };
        test_encode(
            &Variant::from(map),
            &[
                7u8, 2, // 2 elements
                3, 98, 97, 122, // "baz"
                2, 84, // signed(42)
                3, 102, 111, 111, // "foo"
                4, 3, 98, 97, 114, // string("bar")
            ],
        );
    }

    #[test]
    fn coerce() {
        let mut var = Variant::from(true);
        assert!(var.coerce_bool());
        assert_eq!(var.coerce_signed(), 1);
        assert_eq!(var.coerce_unsigned(), 1);

        var = Variant::from(false);
        assert!(!var.coerce_bool());
        assert_eq!(var.coerce_signed(), 0);
        assert_eq!(var.coerce_unsigned(), 0);

        var = Variant::signed(17);
        assert!(var.coerce_bool());
        assert_eq!(var.coerce_signed(), 17);
        assert_eq!(var.coerce_unsigned(), 17);

        var = Variant::signed(-1);
        assert!(var.coerce_bool());
        assert_eq!(var.coerce_signed(), -1);
        assert_eq!(var.coerce_unsigned(), 18_446_744_073_709_551_615);

        var = Variant::unsigned(78u8);
        assert!(var.coerce_bool());
        assert_eq!(var.coerce_signed(), 78);
        assert_eq!(var.coerce_unsigned(), 78);

        var = Variant::unsigned(2u64.pow(63) + 1);
        assert!(var.coerce_bool());
        assert_eq!(var.coerce_signed(), -9_223_372_036_854_775_807);
        assert_eq!(var.coerce_unsigned(), 2u64.pow(63) + 1);

        var = Variant::from("hello");
        assert!(!var.coerce_bool());
        assert_eq!(var.coerce_signed(), 0);
        assert_eq!(var.coerce_unsigned(), 0);
    }
}
