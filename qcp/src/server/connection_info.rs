//! Information about the client connection
// (c) 2024 Ross Younger

/// Parses SSH environment variables to extract the remote IP address.
/// Returns `None` if not found.
pub(super) fn parse_ssh_env(
    env_ssh_connection: Option<&str>,
    env_ssh_client: Option<&str>,
) -> Option<String> {
    if let Some(s) = env_ssh_connection {
        // SSH_CONNECTION: client IP, client port, server IP, server port
        let it = s.split(' ').next();
        if let Some(client) = it {
            if !client.is_empty() {
                return Some(client.to_string());
            }
        }
    }
    if let Some(s) = env_ssh_client {
        // SSH_CLIENT: client IP, client port, server port
        let it = s.split(' ').next();
        if let Some(client) = it {
            if !client.is_empty() {
                return Some(client.to_string());
            }
        }
    }
    None
}
