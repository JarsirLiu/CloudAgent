use crate::error::HttpStreamError;
use crate::transport::SseFrameDecoder;
use reqwest::Response;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

pub struct SseFrameStream {
    rx: mpsc::Receiver<Result<String, HttpStreamError>>,
}

impl SseFrameStream {
    pub async fn recv(&mut self) -> Option<Result<String, HttpStreamError>> {
        self.rx.recv().await
    }
}

pub fn spawn_sse_frame_stream(mut response: Response, idle_timeout: Duration) -> SseFrameStream {
    let (tx, rx) = mpsc::channel(256);

    tokio::spawn(async move {
        let mut decoder = SseFrameDecoder::default();
        let mut saw_frame = false;

        loop {
            match timeout(idle_timeout, response.chunk()).await {
                Ok(Ok(Some(chunk))) => {
                    for frame in decoder.push_chunk(&chunk) {
                        saw_frame = true;
                        if tx.send(Ok(frame)).await.is_err() {
                            return;
                        }
                    }
                }
                Ok(Ok(None)) => {
                    let _ = tx.send(Err(HttpStreamError::ClosedBeforeCompletion)).await;
                    return;
                }
                Ok(Err(err)) => {
                    let _ = tx
                        .send(Err(HttpStreamError::Transport(err.to_string())))
                        .await;
                    return;
                }
                Err(_) => {
                    let _ = tx
                        .send(Err(if saw_frame {
                            HttpStreamError::IdleTimeout
                        } else {
                            HttpStreamError::FirstFrameTimeout
                        }))
                        .await;
                    return;
                }
            }
        }
    });

    SseFrameStream { rx }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    #[tokio::test]
    async fn sse_frame_stream_reports_early_close() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("listener addr");

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept client");
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let read = stream.read(&mut buf).expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let body = "data: hello\n\n";
            let response = concat!(
                "HTTP/1.1 200 OK\r\n",
                "Content-Type: text/event-stream\r\n",
                "Content-Length: 13\r\n",
                "Connection: close\r\n",
                "\r\n"
            );
            stream
                .write_all(response.as_bytes())
                .expect("write headers");
            stream.write_all(body.as_bytes()).expect("write body");
            stream.flush().expect("flush response");
        });

        let response = reqwest::get(format!("http://{addr}"))
            .await
            .expect("send request");
        let mut stream = spawn_sse_frame_stream(response, Duration::from_millis(250));

        assert_eq!(
            stream.recv().await.expect("first frame").expect("ok frame"),
            "hello"
        );
        assert_eq!(
            stream.recv().await.expect("close error"),
            Err(HttpStreamError::ClosedBeforeCompletion)
        );

        server.join().expect("server thread");
    }

    #[tokio::test]
    async fn sse_frame_stream_reports_idle_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("listener addr");

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept client");
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let read = stream.read(&mut buf).expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let response = concat!(
                "HTTP/1.1 200 OK\r\n",
                "Content-Type: text/event-stream\r\n",
                "Transfer-Encoding: chunked\r\n",
                "Connection: keep-alive\r\n",
                "\r\n"
            );
            stream
                .write_all(response.as_bytes())
                .expect("write headers");
            stream.flush().expect("flush response");
            thread::sleep(Duration::from_millis(400));
        });

        let response = reqwest::get(format!("http://{addr}"))
            .await
            .expect("send request");
        let mut stream = spawn_sse_frame_stream(response, Duration::from_millis(100));

        assert_eq!(
            stream.recv().await.expect("timeout error"),
            Err(HttpStreamError::FirstFrameTimeout)
        );

        server.join().expect("server thread");
    }

    #[tokio::test]
    async fn sse_frame_stream_distinguishes_stall_after_first_frame() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("listener addr");

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept client");
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let read = stream.read(&mut buf).expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let body = "data: hello\n\n";
            let response = concat!(
                "HTTP/1.1 200 OK\r\n",
                "Content-Type: text/event-stream\r\n",
                "Transfer-Encoding: chunked\r\n",
                "Connection: keep-alive\r\n",
                "\r\n"
            );
            stream
                .write_all(response.as_bytes())
                .expect("write headers");
            stream
                .write_all(format!("{:X}\r\n", body.len()).as_bytes())
                .expect("write chunk size");
            stream.write_all(body.as_bytes()).expect("write body");
            stream.write_all(b"\r\n").expect("write suffix");
            stream.flush().expect("flush response");
            thread::sleep(Duration::from_millis(400));
        });

        let response = reqwest::get(format!("http://{addr}"))
            .await
            .expect("send request");
        let mut stream = spawn_sse_frame_stream(response, Duration::from_millis(100));

        assert_eq!(
            stream.recv().await.expect("first frame").expect("ok frame"),
            "hello"
        );
        assert_eq!(
            stream.recv().await.expect("stall error"),
            Err(HttpStreamError::IdleTimeout)
        );

        server.join().expect("server thread");
    }
}
