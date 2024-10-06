# QCP session protocol wire format definitions
@0xb2af08ed873f6840;

# A command from client to server.
# Server must respond with a Response before anything else can happen on this connection.
struct Command {
    args : union {
        get@0: GetCmdArgs;
        # Retrieves a file. This may fail if the file does not exist or the user doesn't have read permission.
        # Client -> Server: Command (Get)
        # S->C: Response, FileHeader, file data, FileTrailer.
        # Client closes the stream after transfer.
        # If the client needs to abort transfer, it closes the stream.
        # If the server needs to abort transfer, it closes the stream.

        put@1: PutCmdArgs;
        # Sends a file. This may fail for permissions or if the containing directory doesn't exist.
        # Client -> Server: Command (Put)
        # S->C: Response (to the command)
        # (if not OK - close stream or send another command)
        # C->S: FileHeader, file data, FileTrailer
        # S->C: Response (showing transfer status)
        # Then close the stream.
        # If the server needs to abort the transfer:
        # S->C: Status (explaining why), then close the stream.
    }

    struct GetCmdArgs {
        filename @0 : Text;
        # Filename is a file name only, without any directory components
    }
    struct PutCmdArgs {
        filename @0 : Text;
        # Filename is a file name only, without any directory components
    }
}

# Server's response to a Command
struct Response {
    status @0: Status; # Status code
    message @1: Text; # Human-readable message explaining the situation (may be empty)
}

enum Status {
    ok @0;
    fileNotFound @1;
    incorrectPermissions @2;
    directoryDoesNotExist @3;
    ioError @4;
    diskFull @5;
    notYetImplemented @6;
    itIsADirectory @7;
}

struct FileHeader {
    size @0 : UInt64;
    filename @1 : Text;
}

struct FileTrailer {
    # empty for now, this will probably have a checksum later
}
