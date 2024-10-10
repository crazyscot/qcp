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
    warning @3: Text; # If present, a warning message to be relayed to a human
    bandwidthInfo @4: Text; # Reports the server's active bandwidth configuration
}

struct ClosedownReport {
    finalCongestionWindow @0: UInt64;
    sentPackets @1: UInt64;
    lostPackets @2: UInt64;
    lostBytes @3: UInt64;
    congestionEvents @4: UInt64;
    blackHoles @5: UInt64;
    sentBytes @6: UInt64;
}
