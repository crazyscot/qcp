# QCP control protocol wire format definitions
@0xeb4bca415866a714;

struct ClientMessage {
    cert @0: Data; # Client's self-signed certificate (DER)
    connectionType @1: ConnectionType; # Specified by client

    enum ConnectionType {
        ipv4 @0;
        ipv6 @1;
    }
}

struct ServerMessage {
    port @0: UInt16; # UDP port the server has bound to
    cert @1: Data; # Server's self-signed certificate (DER)
    name @2: Text; # Name in the server cert (this saves us having to unpick it from the certificate)
}
