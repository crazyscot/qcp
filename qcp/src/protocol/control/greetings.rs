//! ## Client/Server Greetings
// (c) 2024-25 Ross Younger

use crate::protocol::prelude::*;

////////////////////////////////////////////////////////////////////////////////////////
// CLIENT GREETING

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
/// The initial message from client to server.
///
/// We have to send this message without knowing what version the server supports.
pub struct ClientGreeting {
    /// Protocol compatibility version identifier
    ///
    /// This identifies the client's maximum supported protocol sub-version.
    ///
    /// N.B. This is not sent as an enum to avoid breaking the server when we have a newer version!
    pub compatibility: u16,
    /// Requests the remote emit debug information over the control channel (stderr).
    pub debug: bool,
    /// Extension field, reserved for future expansion; for now, must be set to 0
    pub extension: u8,
}
impl ProtocolMessage for ClientGreeting {
    const WIRE_ENCODING_LIMIT: u32 = 4_096;
}

////////////////////////////////////////////////////////////////////////////////////////
// SERVER GREETING

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
/// The initial message from server to client.
///
/// Like [`ClientGreeting`] this is designed to be sent without knowing what version the client supports.
pub struct ServerGreeting {
    /// Protocol compatibility version identifier
    ///
    /// This identifies the client's maximum supported protocol sub-version.
    ///
    /// N.B. This is not sent as an enum to avoid breaking the server when we have a newer version!
    pub compatibility: u16,
    /// Extension field, reserved for future expansion; for now, must be set to 0
    pub extension: u8,
}
impl ProtocolMessage for ServerGreeting {
    const WIRE_ENCODING_LIMIT: u32 = 4_096;
}

////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use super::{ClientGreeting, ServerGreeting};
    use crate::protocol::prelude::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn serialize_client_greeting() {
        let msg = ClientGreeting {
            compatibility: 1,
            debug: false,
            extension: 0,
        };
        let wire = msg.to_vec().unwrap();
        let deser = ClientGreeting::from_slice(&wire).unwrap();
        assert_eq!(msg, deser);
    }

    #[test]
    fn serialize_server_greeting() {
        let msg = ServerGreeting {
            compatibility: 1,
            extension: 0,
        };
        let wire = msg.to_vec().unwrap();
        let deser = ServerGreeting::from_slice(&wire).unwrap();
        assert_eq!(msg, deser);
    }

    #[test]
    fn wire_marshalling_client_greeting() {
        // This message is critical to the entire protocol. It cannot change without breaking compatibility.
        let msg = ClientGreeting {
            compatibility: 1,
            debug: true,
            extension: 3,
        };
        let wire = msg.to_vec().unwrap();
        let expected = b"\x01\x00\x01\x03".to_vec();
        assert_eq!(wire, expected);
    }

    #[test]
    fn wire_marshalling_server_greeting() {
        // This message is critical to the entire protocol. It cannot change without breaking compatibility.
        let msg = ServerGreeting {
            compatibility: 1,
            extension: 4,
        };
        let wire = msg.to_vec().unwrap();
        let expected = b"\x01\x00\x04".to_vec();
        assert_eq!(wire, expected);
    }
}
