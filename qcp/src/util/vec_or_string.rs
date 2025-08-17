//! Deserialisation sugar
//!
// (c) 2024 Ross Younger

/// Helper type for deserialisation.
///
/// The underlying data is a `Vec<String>`, however if it is of size 1 it _may_ be represented as a String.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
#[serde(into = "Vec<String>")]
pub enum VecOrString {
    /// contents are a string
    String(String),
    /// contents are a vector
    Vec(Vec<String>),
}

impl VecOrString {
    /// Clones the contents to make a vector
    #[must_use]
    pub fn to_vec_owned(&self) -> Vec<String> {
        match self {
            VecOrString::String(s) => vec![s.clone()],
            VecOrString::Vec(v) => v.to_owned(),
        }
    }

    /// Extracts the contents to a vector, allocating one if necessary.
    #[must_use]
    pub fn to_vec(self) -> Vec<String> {
        match self {
            VecOrString::String(s) => {
                vec![s]
            }
            VecOrString::Vec(v) => v,
        }
    }
}

impl From<VecOrString> for Vec<String> {
    fn from(value: VecOrString) -> Self {
        value.to_vec()
    }
}

impl From<Vec<String>> for VecOrString {
    fn from(value: Vec<String>) -> Self {
        VecOrString::Vec(value)
    }
}

impl From<&[&str]> for VecOrString {
    fn from(value: &[&str]) -> Self {
        VecOrString::Vec(value.iter().map(|s| String::from(*s)).collect::<Vec<_>>())
    }
}

impl From<String> for VecOrString {
    fn from(value: String) -> Self {
        VecOrString::String(value)
    }
}

impl From<&str> for VecOrString {
    fn from(value: &str) -> Self {
        VecOrString::String(value.to_string())
    }
}

impl Default for VecOrString {
    fn default() -> Self {
        VecOrString::Vec(vec![])
    }
}

impl PartialEq for VecOrString {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(l0), Self::String(r0)) => l0 == r0,
            (Self::Vec(l0), Self::Vec(r0)) => l0 == r0,
            (Self::String(l0), Self::Vec(r0)) => r0.len() == 1 && l0 == &r0[0],
            (Self::Vec(l0), Self::String(r0)) => l0.len() == 1 && &l0[0] == r0,
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use crate::util::VecOrString;
    use pretty_assertions::assert_eq;

    #[test]
    fn comparison() {
        assert_eq!(VecOrString::from("a"), VecOrString::from("a".to_string()));
        assert_eq!(
            VecOrString::from(vec!["b".into()]),
            VecOrString::from(vec!["b".into()]),
        );
        assert_eq!(VecOrString::from("c"), VecOrString::from(vec!["c".into()]),);
        assert_eq!(VecOrString::from(vec!["d".into()]), VecOrString::from("d"),);
    }
}
