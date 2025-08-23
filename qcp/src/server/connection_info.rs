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
        if let Some(client) = it
            && !client.is_empty()
        {
            return Some(client.to_string());
        }
    }
    if let Some(s) = env_ssh_client {
        // SSH_CLIENT: client IP, client port, server port
        let it = s.split(' ').next();
        if let Some(client) = it
            && !client.is_empty()
        {
            return Some(client.to_string());
        }
    }
    None
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::parse_ssh_env;

    #[test]
    fn test_parse_ssh_env_connection() {
        let env_ssh_connection = Some("192.168.1.2 12345 192.168.1.1 22");
        let env_ssh_client = None;
        let result = parse_ssh_env(env_ssh_connection, env_ssh_client);
        assert_eq!(result.as_deref(), Some("192.168.1.2"));
    }

    #[test]
    fn test_parse_ssh_env_client() {
        let env_ssh_connection = None;
        let env_ssh_client = Some("10.0.0.5 54321 22");
        let result = parse_ssh_env(env_ssh_connection, env_ssh_client);
        assert_eq!(result.as_deref(), Some("10.0.0.5"));
    }

    #[test]
    fn test_parse_ssh_env_none() {
        let env_ssh_connection = None;
        let env_ssh_client = None;
        let result = parse_ssh_env(env_ssh_connection, env_ssh_client);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_ssh_env_empty_string() {
        let env_ssh_connection = Some("");
        let env_ssh_client = Some("");
        let result = parse_ssh_env(env_ssh_connection, env_ssh_client);
        assert_eq!(result, None);
    }
}
