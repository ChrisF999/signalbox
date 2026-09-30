//! The bot's network side: a dev-auth login over plain HTTP and a WebSocket
//! carrying `ClientFrame`s out and `ServerFrame`s in. Plain `http://` only:
//! dev login exists only in test builds of the server, which are local.

use futures_util::{SinkExt, StreamExt};
use protocol::{ClientFrame, FrameError, ServerFrame};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bad url `{0}` (want http://host:port)")]
    Url(String),
    #[error("bad http response: {0}")]
    Http(String),
    #[error("http status {0}")]
    Status(u16),
    #[error("websocket: {0}")]
    Ws(tungstenite::Error),
    #[error("bad frame from the server: {0}")]
    Frame(#[from] FrameError),
    #[error("timed out: {0}")]
    Timeout(String),
}

impl From<tungstenite::Error> for NetError {
    fn from(e: tungstenite::Error) -> NetError {
        match e {
            tungstenite::Error::Http(r) => NetError::Status(r.status().as_u16()),
            e => NetError::Ws(e),
        }
    }
}

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    /// Header names lower-cased, in order.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }
}

fn host_port(base: &str) -> Result<&str, NetError> {
    base.strip_prefix("http://").map(|h| h.trim_end_matches('/')).filter(|h| !h.is_empty()).ok_or_else(|| NetError::Url(base.into()))
}

/// A bare HTTP/1.0 GET (no redirects followed), enough for the dev login
/// and for tests. `base` is `http://host:port`; `path` starts with `/`.
pub async fn http_get(base: &str, path: &str, cookie: Option<&str>) -> Result<HttpResponse, NetError> {
    let host = host_port(base)?;
    let mut stream = TcpStream::connect(host).await?;
    let mut req = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\n");
    if let Some(c) = cookie {
        req.push_str(&format!("Cookie: {c}\r\n"));
    }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).await?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await?;
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").ok_or_else(|| NetError::Http("no end of headers".into()))?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status = status_line
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| NetError::Http(format!("status line `{status_line}`")))?;
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    Ok(HttpResponse { status, headers, body: body.to_string() })
}

/// Percent-encode everything but unreserved characters.
pub fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Log in through a dev-auth server's `/auth/dev`; returns the session
/// cookie as `name=value`, ready for a `Cookie` header.
pub async fn dev_login(base: &str, user: &str) -> Result<String, NetError> {
    let r = http_get(base, &format!("/auth/dev?user={}", url_encode(user)), None).await?;
    if !(200..400).contains(&r.status) {
        return Err(NetError::Status(r.status));
    }
    let set = r.header("set-cookie").ok_or_else(|| NetError::Http("no session cookie".into()))?;
    Ok(set.split(';').next().unwrap_or_default().trim().to_string())
}

/// One WebSocket connection to the front.
pub struct Conn {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl Conn {
    /// Open `/ws` on `base` (`http://host:port`), sending `cookie` if given.
    /// A refused upgrade is `NetError::Status(code)`.
    pub async fn connect(base: &str, cookie: Option<&str>) -> Result<Conn, NetError> {
        let url = format!("ws://{}/ws", host_port(base)?);
        let mut req = url.into_client_request()?;
        if let Some(c) = cookie {
            let v = HeaderValue::from_str(c).map_err(|_| NetError::Http("cookie is not a header value".into()))?;
            req.headers_mut().insert("cookie", v);
        }
        let (ws, _) = connect_async(req).await?;
        Ok(Conn { ws })
    }

    pub async fn send(&mut self, frame: &ClientFrame) -> Result<(), NetError> {
        self.send_text(frame.to_json()).await
    }

    /// Send any text (tests use it for malformed frames).
    pub async fn send_text(&mut self, text: String) -> Result<(), NetError> {
        self.ws.send(Message::text(text)).await?;
        Ok(())
    }

    pub async fn send_binary(&mut self, bytes: Vec<u8>) -> Result<(), NetError> {
        self.ws.send(Message::binary(bytes)).await?;
        Ok(())
    }

    /// The next frame; `Ok(None)` once the server has closed the socket.
    pub async fn recv(&mut self) -> Result<Option<ServerFrame>, NetError> {
        while let Some(m) = self.ws.next().await {
            match m {
                Ok(Message::Text(t)) => return Ok(Some(ServerFrame::from_json(t.as_str())?)),
                Ok(Message::Close(_)) => return Ok(None),
                Ok(_) => continue,
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => return Ok(None),
                Err(tungstenite::Error::Protocol(tungstenite::error::ProtocolError::ResetWithoutClosingHandshake)) => {
                    return Ok(None);
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(None)
    }

    pub async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }
}
