//! Remote console over the Source RCON protocol (what Pterodactyl, mcrcon and most hosting
//! panels speak). Each packet is: i32 length, i32 request id, i32 type, a text body, two NULs,
//! all little-endian. Type 3 logs in with the password, type 2 runs a console command.

use crate::players::Server;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const AUTH: i32 = 3;
const EXEC: i32 = 2;
const AUTH_REPLY: i32 = 2;
const RESPONSE: i32 = 0;

fn packet(id: i32, kind: i32, body: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(14 + body.len());
    out.extend_from_slice(&(10 + body.len() as i32).to_le_bytes());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(body.as_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}

/// Starts listening if `enable-rcon` is on and a password is set.
pub fn start(server: Server) {
    let cfg = crate::config::get();
    if !cfg.enable_rcon {
        return;
    }
    if cfg.rcon_password.is_empty() {
        println!("RCON is on but rcon.password is empty: not starting it. Set a password in server.properties.");
        return;
    }
    let (port, password) = (cfg.rcon_port, cfg.rcon_password.clone());
    tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
            Ok(l) => l,
            Err(e) => return println!("RCON couldn't listen on port {port}: {e}"),
        };
        println!("RCON listening on port {port}");
        loop {
            let Ok((mut sock, addr)) = listener.accept().await else { continue };
            let (server, password) = (server.clone(), password.clone());
            tokio::spawn(async move {
                let mut authed = false;
                loop {
                    let mut len = [0u8; 4];
                    if sock.read_exact(&mut len).await.is_err() {
                        return;
                    }
                    let len = i32::from_le_bytes(len);
                    if !(10..=4106).contains(&len) {
                        return; // not RCON, or an oversized packet
                    }
                    let mut buf = vec![0u8; len as usize];
                    if sock.read_exact(&mut buf).await.is_err() {
                        return;
                    }
                    let id = i32::from_le_bytes(buf[0..4].try_into().unwrap_or_default());
                    let kind = i32::from_le_bytes(buf[4..8].try_into().unwrap_or_default());
                    let body = String::from_utf8_lossy(&buf[8..buf.len() - 2]).into_owned();
                    let reply = match kind {
                        AUTH => {
                            authed = body == password;
                            if !authed {
                                println!("RCON: wrong password from {addr}");
                            }
                            packet(if authed { id } else { -1 }, AUTH_REPLY, "")
                        }
                        EXEC if authed => {
                            let line = body.trim().trim_start_matches('/');
                            println!("RCON ({addr}): {line}");
                            let args: Vec<&str> = line.split_whitespace().collect();
                            let text = match crate::commands::run(&server, crate::commands::CONSOLE, &args) {
                                Some(r) => crate::strip_colors(&r),
                                None => "Unknown command. Type \"help\" for help.".into(),
                            };
                            packet(id, RESPONSE, &text)
                        }
                        _ => packet(-1, AUTH_REPLY, ""),
                    };
                    if sock.write_all(&reply).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn packet_layout() {
        let p = super::packet(7, 0, "hi");
        assert_eq!(&p[0..4], &12i32.to_le_bytes());
        assert_eq!(&p[4..8], &7i32.to_le_bytes());
        assert_eq!(&p[12..14], b"hi");
        assert_eq!(&p[14..], &[0, 0]);
    }
}
