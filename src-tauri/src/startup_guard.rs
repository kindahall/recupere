#[cfg(all(debug_assertions, target_os = "macos"))]
use std::process::Command;
#[cfg(any(debug_assertions, test))]
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream},
    time::Duration,
};

#[cfg(any(debug_assertions, test))]
const DEV_FRONTEND_URL: &str = "http://localhost:1420";
#[cfg(any(debug_assertions, test))]
const DEV_FRONTEND_PORT: u16 = 1420;
#[cfg(any(debug_assertions, test))]
const DEV_FRONTEND_TIMEOUT_MS: u64 = 200;

#[cfg(debug_assertions)]
pub fn ensure_dev_frontend_available() -> Result<(), String> {
    if std::env::var_os("RECUPERE_SKIP_DEV_FRONTEND_CHECK").is_some() {
        return Ok(());
    }

    if dev_frontend_reachable() {
        return Ok(());
    }

    Err(build_missing_dev_frontend_message(DEV_FRONTEND_URL))
}

#[cfg(debug_assertions)]
pub fn report_startup_error(message: &str) {
    eprintln!("[recupere] startup_guard: {message}");

    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display alert \"Récupère\" message \"{}\" as critical buttons {{\"OK\"}} default button \"OK\"",
            escape_applescript_literal(message)
        );
        let _ = Command::new("osascript").arg("-e").arg(script).status();
    }
}

#[cfg(any(debug_assertions, test))]
fn dev_frontend_reachable() -> bool {
    let timeout = Duration::from_millis(DEV_FRONTEND_TIMEOUT_MS);
    let candidates = [
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEV_FRONTEND_PORT),
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), DEV_FRONTEND_PORT),
    ];

    candidates
        .iter()
        .copied()
        .any(|address| tcp_endpoint_reachable(address, timeout))
}

#[cfg(any(debug_assertions, test))]
fn build_missing_dev_frontend_message(dev_url: &str) -> String {
    format!(
        "The development frontend is not reachable at {dev_url}. Start the app with `npm run tauri dev`, or run `npm run dev` before launching `target/debug/recupere`."
    )
}

#[cfg(any(debug_assertions, test))]
fn tcp_endpoint_reachable(address: SocketAddr, timeout: Duration) -> bool {
    TcpStream::connect_timeout(&address, timeout).is_ok()
}

#[cfg(all(debug_assertions, target_os = "macos"))]
fn escape_applescript_literal(message: &str) -> String {
    message.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn build_missing_dev_frontend_message_mentions_supported_launch_commands() {
        let message = build_missing_dev_frontend_message("http://localhost:1420");
        assert!(message.contains("localhost:1420"));
        assert!(message.contains("npm run tauri dev"));
        assert!(message.contains("npm run dev"));
        assert!(message.contains("target/debug/recupere"));
    }

    #[test]
    fn tcp_endpoint_reachable_detects_a_listening_local_port() {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test listener should bind");
        let address = listener
            .local_addr()
            .expect("test listener should expose a local address");

        assert!(tcp_endpoint_reachable(
            address,
            Duration::from_millis(DEV_FRONTEND_TIMEOUT_MS)
        ));
    }
}
