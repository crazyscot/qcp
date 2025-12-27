//! Command structures for GET and PUT
// (c) 2025 Ross Younger

use crate::protocol::session::prelude::*;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Default, Clone)]
/// Arguments for the `GET` command
pub struct GetArgs {
    /// This is a file name, with leading directory components as required
    pub filename: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Default, Clone)]
/// Arguments for the `GET2` command.
/// This was introduced in qcp 0.5 with `VersionCompatibility=V2`.
pub struct Get2Args {
    /// This is a file name, with leading directory components as required
    pub filename: String,

    /// Extended options for the GET command
    ///
    /// Supported options: [`CommandParam::PreserveMetadata`]
    pub options: Vec<TaggedData<CommandParam>>,
}
impl From<GetArgs> for Get2Args {
    fn from(v1: GetArgs) -> Self {
        Self {
            filename: v1.filename,
            options: vec![],
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Default, Clone)]
/// Arguments for the `PUT` command
pub struct PutArgs {
    /// This is the destination file or directory name, with leading directory components as required.
    /// If it is a directory name, the filename given in the protocol `FileHeader` is appended.
    pub filename: String,
}
#[derive(Serialize, Deserialize, PartialEq, Debug, Default, Clone)]
/// Arguments for the `PUT2` command.
/// This was introduced in qcp 0.5 with `VersionCompatibility=V2`.
pub struct Put2Args {
    /// This is the destination file or directory name, with leading directory components as required.
    /// If it is a directory name, the filename given in the protocol `FileHeader` is appended.
    pub filename: String,

    /// Extended options for the PUT command
    ///
    /// Supported options: [`CommandParam::PreserveMetadata`]
    pub options: Vec<TaggedData<CommandParam>>,
}
impl From<PutArgs> for Put2Args {
    fn from(v1: PutArgs) -> Self {
        Self {
            filename: v1.filename,
            options: vec![],
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use crate::protocol::session::Command;
    use crate::protocol::session::prelude::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn wire_marshalling_command_get() {
        let cmd = Command::Get(super::GetArgs {
            filename: "myfile".to_string(),
        });
        let wire = cmd.to_vec().unwrap();
        let expected = b"\x00\x06myfile".to_vec();
        assert_eq!(wire, expected);
    }

    #[test]
    fn wire_marshalling_command_put() {
        let cmd = Command::Put(super::PutArgs {
            filename: "myfile2".to_string(),
        });
        let wire = cmd.to_vec().unwrap();
        let expected = b"\x01\x07myfile2".to_vec();
        assert_eq!(wire, expected);
    }
}
