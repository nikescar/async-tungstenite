//! `asupersync` integration.
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::client::{Request, Response};
use tungstenite::handshake::server::{Callback, NoCallback};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::Error;

use asupersync::net::TcpStream;

use super::{domain, port, WebSocketStream};

use futures_io::{AsyncRead, AsyncWrite};

#[cfg(feature = "asupersync-tls")]
use crate::stream::Stream as StreamSwitcher;

#[cfg(feature = "asupersync-tls")]
mod tls {
    use super::*;
    use asupersync::tls::{TlsConnector, TlsConnectorBuilder};
    use asupersync::tls::TlsStream as AsupersyncTlsStream;
    use tungstenite::client::uri_mode;
    use tungstenite::stream::Mode;

    use super::AsupersyncAdapter;

    /// A stream that might be protected with TLS.
    pub type MaybeTlsStream<S> = StreamSwitcher<AsupersyncAdapter<S>, AsupersyncAdapter<AsupersyncTlsStream<S>>>;

    /// Type alias for streams returned by `client_async` functions (may be TLS or plain).
    pub type AutoStream<S> = MaybeTlsStream<S>;

    /// TLS connector type for asupersync.
    pub type Connector = TlsConnector;

    async fn wrap_stream<S>(
        socket: S,
        domain: String,
        connector: Option<Connector>,
        mode: Mode,
    ) -> Result<AutoStream<S>, Error>
    where
        S: asupersync::io::AsyncRead + asupersync::io::AsyncWrite + Unpin,
    {
        match mode {
            Mode::Plain => Ok(StreamSwitcher::Plain(AsupersyncAdapter::new(socket))),
            Mode::Tls => {
                let connector = if let Some(connector) = connector {
                    connector
                } else {
                    #[cfg(feature = "asupersync-tls-webpki-roots")]
                    let connector = TlsConnectorBuilder::new()
                        .with_webpki_roots()
                        .build()
                        .map_err(|e| Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

                    #[cfg(all(
                        feature = "asupersync-tls-native-roots",
                        not(feature = "asupersync-tls-webpki-roots")
                    ))]
                    let connector = TlsConnectorBuilder::new()
                        .with_native_roots()
                        .map_err(|e| Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?
                        .build()
                        .map_err(|e| Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

                    #[cfg(not(any(
                        feature = "asupersync-tls-webpki-roots",
                        feature = "asupersync-tls-native-roots"
                    )))]
                    return Err(Error::Url(
                        tungstenite::error::UrlError::TlsFeatureNotEnabled,
                    ));

                    connector
                };

                // Connect with TLS using asupersync's native traits
                let tls_stream = connector
                    .connect(&domain, socket)
                    .await
                    .map_err(|e| Error::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

                // Wrap the TLS stream in AsupersyncAdapter for futures_io compatibility
                Ok(StreamSwitcher::Tls(AsupersyncAdapter::new(tls_stream)))
            }
        }
    }

    /// Creates a WebSocket handshake from a request and a stream,
    /// upgrading the stream to TLS if required and using the given
    /// connector and WebSocket configuration.
    pub async fn client_async_tls_with_connector_and_config<R, S>(
        request: R,
        stream: S,
        connector: Option<Connector>,
        config: Option<WebSocketConfig>,
    ) -> Result<(WebSocketStream<AutoStream<S>>, Response), Error>
    where
        R: IntoClientRequest + Unpin,
        S: asupersync::io::AsyncRead + asupersync::io::AsyncWrite + Unpin,
        AutoStream<S>: Unpin,
    {
        let request: Request = request.into_client_request()?;
        let domain = crate::domain(&request)?;
        let mode = uri_mode(request.uri())?;

        let stream = wrap_stream(stream, domain, connector, mode).await?;
        crate::client_async_with_config(request, stream, config).await
    }
}

#[cfg(not(feature = "asupersync-tls"))]
mod tls {
    use super::*;

    /// Type alias for streams when TLS is not enabled.
    pub type AutoStream<S> = AsupersyncAdapter<S>;
    
    /// Dummy connector type when TLS is not enabled.
    pub type Connector = ();

    /// Internal function to handle client connections without TLS support.
    /// Returns an error if a TLS connection is attempted.
    pub async fn client_async_tls_with_connector_and_config<R, S>(
        request: R,
        stream: S,
        _connector: Option<Connector>,
        config: Option<WebSocketConfig>,
    ) -> Result<(WebSocketStream<AutoStream<S>>, Response), Error>
    where
        R: IntoClientRequest + Unpin,
        S: asupersync::io::AsyncRead + asupersync::io::AsyncWrite + Unpin,
        AutoStream<S>: Unpin,
    {
        use tungstenite::client::uri_mode;
        use tungstenite::stream::Mode;

        let request: Request = request.into_client_request()?;
        let _domain = crate::domain(&request)?;
        let mode = uri_mode(request.uri())?;

        match mode {
            Mode::Plain => {
                crate::client_async_with_config(request, AsupersyncAdapter::new(stream), config).await
            }
            Mode::Tls => Err(Error::Url(
                tungstenite::error::UrlError::TlsFeatureNotEnabled,
            )),
        }
    }
}

pub use self::tls::{client_async_tls_with_connector_and_config, AutoStream, Connector};

/// Creates a WebSocket handshake from a request and a stream.
/// For convenience, the user may call this with a url string, a URL,
/// or a `Request`. Calling with `Request` allows the user to add
/// a WebSocket protocol or other custom headers.
///
/// Internally, this custom creates a handshake representation and returns
/// a future representing the resolution of the WebSocket handshake. The
/// returned future will resolve to either `WebSocketStream<S>` or `Error`
/// depending on whether the handshake is successful.
///
/// This is typically used for clients who have already established, for
/// example, a TCP connection to the remote server.
pub async fn client_async<'a, R, S>(
    request: R,
    stream: S,
) -> Result<(WebSocketStream<AsupersyncAdapter<S>>, Response), Error>
where
    R: IntoClientRequest + Unpin,
    S: asupersync::io::AsyncRead + asupersync::io::AsyncWrite + Unpin,
{
    client_async_with_config(request, stream, None).await
}

/// The same as `client_async()` but the one can specify a websocket configuration.
/// Please refer to `client_async()` for more details.
pub async fn client_async_with_config<'a, R, S>(
    request: R,
    stream: S,
    config: Option<WebSocketConfig>,
) -> Result<(WebSocketStream<AsupersyncAdapter<S>>, Response), Error>
where
    R: IntoClientRequest + Unpin,
    S: asupersync::io::AsyncRead + asupersync::io::AsyncWrite + Unpin,
{
    crate::client_async_with_config(request, AsupersyncAdapter::new(stream), config).await
}

/// Accepts a new WebSocket connection with the provided stream.
///
/// This function will internally call `server::accept` to create a
/// handshake representation and returns a future representing the
/// resolution of the WebSocket handshake. The returned future will resolve
/// to either `WebSocketStream<S>` or `Error` depending if it's successful
/// or not.
///
/// This is typically used after a socket has been accepted from a
/// `TcpListener`. That socket is then passed to this function to perform
/// the server half of the accepting a client's websocket connection.
pub async fn accept_async<S>(stream: S) -> Result<WebSocketStream<AsupersyncAdapter<S>>, Error>
where
    S: asupersync::io::AsyncRead + asupersync::io::AsyncWrite + Unpin,
{
    accept_hdr_async(stream, NoCallback).await
}

/// The same as `accept_async()` but the one can specify a websocket configuration.
/// Please refer to `accept_async()` for more details.
pub async fn accept_async_with_config<S>(
    stream: S,
    config: Option<WebSocketConfig>,
) -> Result<WebSocketStream<AsupersyncAdapter<S>>, Error>
where
    S: asupersync::io::AsyncRead + asupersync::io::AsyncWrite + Unpin,
{
    accept_hdr_async_with_config(stream, NoCallback, config).await
}

/// Accepts a new WebSocket connection with the provided stream.
///
/// This function does the same as `accept_async()` but accepts an extra callback
/// for header processing. The callback receives headers of the incoming
/// requests and is able to add extra headers to the reply.
pub async fn accept_hdr_async<S, C>(
    stream: S,
    callback: C,
) -> Result<WebSocketStream<AsupersyncAdapter<S>>, Error>
where
    S: asupersync::io::AsyncRead + asupersync::io::AsyncWrite + Unpin,
    C: Callback + Unpin,
{
    accept_hdr_async_with_config(stream, callback, None).await
}

/// The same as `accept_hdr_async()` but the one can specify a websocket configuration.
/// Please refer to `accept_hdr_async()` for more details.
pub async fn accept_hdr_async_with_config<S, C>(
    stream: S,
    callback: C,
    config: Option<WebSocketConfig>,
) -> Result<WebSocketStream<AsupersyncAdapter<S>>, Error>
where
    S: asupersync::io::AsyncRead + asupersync::io::AsyncWrite + Unpin,
    C: Callback + Unpin,
{
    crate::accept_hdr_async_with_config(AsupersyncAdapter::new(stream), callback, config).await
}

/// Type alias for the stream type of the `client_async()` functions.
pub type ClientStream<S> = AutoStream<S>;

/// Type alias for the stream type of the `connect_async()` functions.
pub type ConnectStream = ClientStream<TcpStream>;

#[cfg(any(feature = "asupersync-tls-native-roots", feature = "asupersync-tls-webpki-roots"))]
/// Creates a WebSocket handshake from a request and a stream,
/// upgrading the stream to TLS if required.
pub async fn client_async_tls<R, S>(
    request: R,
    stream: S,
) -> Result<(WebSocketStream<ClientStream<S>>, Response), Error>
where
    R: IntoClientRequest + Unpin,
    S: asupersync::io::AsyncRead + asupersync::io::AsyncWrite + Unpin,
    AutoStream<S>: Unpin,
{
    client_async_tls_with_connector_and_config(request, stream, None, None).await
}

#[cfg(any(feature = "asupersync-tls-native-roots", feature = "asupersync-tls-webpki-roots"))]
/// Creates a WebSocket handshake from a request and a stream,
/// upgrading the stream to TLS if required and using the given
/// WebSocket configuration.
pub async fn client_async_tls_with_config<R, S>(
    request: R,
    stream: S,
    config: Option<WebSocketConfig>,
) -> Result<(WebSocketStream<ClientStream<S>>, Response), Error>
where
    R: IntoClientRequest + Unpin,
    S: asupersync::io::AsyncRead + asupersync::io::AsyncWrite + Unpin,
    AutoStream<S>: Unpin,
{
    client_async_tls_with_connector_and_config(request, stream, None, config).await
}

#[cfg(any(feature = "asupersync-tls-native-roots", feature = "asupersync-tls-webpki-roots"))]
/// Creates a WebSocket handshake from a request and a stream,
/// upgrading the stream to TLS if required and using the given connector.
pub async fn client_async_tls_with_connector<R, S>(
    request: R,
    stream: S,
    connector: Option<Connector>,
) -> Result<(WebSocketStream<ClientStream<S>>, Response), Error>
where
    R: IntoClientRequest + Unpin,
    S: asupersync::io::AsyncRead + asupersync::io::AsyncWrite + Unpin,
    AutoStream<S>: Unpin,
{
    client_async_tls_with_connector_and_config(request, stream, connector, None).await
}

/// Connect to a given URL.
pub async fn connect_async<R>(
    request: R,
) -> Result<(WebSocketStream<ConnectStream>, Response), Error>
where
    R: IntoClientRequest + Unpin,
{
    connect_async_with_config(request, None).await
}

/// Connect to a given URL with a given WebSocket configuration.
pub async fn connect_async_with_config<R>(
    request: R,
    config: Option<WebSocketConfig>,
) -> Result<(WebSocketStream<ConnectStream>, Response), Error>
where
    R: IntoClientRequest + Unpin,
{
    use std::net::ToSocketAddrs;
    
    let request: Request = request.into_client_request()?;

    let domain = domain(&request)?;
    let port = port(&request)?;

    // Resolve the address to SocketAddr to avoid lifetime issues
    let addr = (domain.as_str(), port)
        .to_socket_addrs()
        .map_err(Error::Io)?
        .next()
        .ok_or_else(|| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Could not resolve address",
            ))
        })?;

    let socket = TcpStream::connect(addr).await.map_err(Error::Io)?;
    
    // Pass the raw TcpStream - the TLS function expects asupersync::io traits
    client_async_tls_with_connector_and_config(request, socket, None, config).await
}

#[cfg(any(feature = "asupersync-tls-native-roots", feature = "asupersync-tls-webpki-roots"))]
/// Connect to a given URL using the provided TLS connector.
pub async fn connect_async_with_tls_connector<R>(
    request: R,
    connector: Option<Connector>,
) -> Result<(WebSocketStream<ConnectStream>, Response), Error>
where
    R: IntoClientRequest + Unpin,
{
    connect_async_with_tls_connector_and_config(request, connector, None).await
}

#[cfg(any(feature = "asupersync-tls-native-roots", feature = "asupersync-tls-webpki-roots"))]
/// Connect to a given URL using the provided TLS connector and WebSocket configuration.
pub async fn connect_async_with_tls_connector_and_config<R>(
    request: R,
    connector: Option<Connector>,
    config: Option<WebSocketConfig>,
) -> Result<(WebSocketStream<ConnectStream>, Response), Error>
where
    R: IntoClientRequest + Unpin,
{
    use std::net::ToSocketAddrs;

    let request: Request = request.into_client_request()?;

    let domain = domain(&request)?;
    let port = port(&request)?;

    // Resolve the address to SocketAddr to avoid lifetime issues
    let addr = (domain.as_str(), port)
        .to_socket_addrs()
        .map_err(Error::Io)?
        .next()
        .ok_or_else(|| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Could not resolve address",
            ))
        })?;

    let socket = TcpStream::connect(addr).await.map_err(Error::Io)?;
    
    // Pass the raw TcpStream - the TLS function expects asupersync::io traits
    client_async_tls_with_connector_and_config(request, socket, connector, config).await
}

use std::pin::Pin;
use std::task::{Context, Poll};

pin_project_lite::pin_project! {
    /// Adapter for `asupersync::io::AsyncRead` and `asupersync::io::AsyncWrite` to provide
    /// the variants from the `futures` crate and the other way around.
    #[derive(Debug, Clone)]
    pub struct AsupersyncAdapter<T> {
        #[pin]
        inner: T,
    }
}

impl<T> AsupersyncAdapter<T> {
    /// Creates a new `AsupersyncAdapter` wrapping the provided value.
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    /// Consumes this `AsupersyncAdapter`, returning the underlying value.
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// Get a reference to the underlying value.
    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    /// Get a mutable reference to the underlying value.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: asupersync::io::AsyncRead> AsyncRead for AsupersyncAdapter<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        let mut buf = asupersync::io::ReadBuf::new(buf);
        match self.project().inner.poll_read(cx, &mut buf)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready(_) => Poll::Ready(Ok(buf.filled().len())),
        }
    }
}

impl<T: asupersync::io::AsyncWrite> AsyncWrite for AsupersyncAdapter<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        self.project().inner.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        self.project().inner.poll_shutdown(cx)
    }
}

impl<T: AsyncRead> asupersync::io::AsyncRead for AsupersyncAdapter<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut asupersync::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let slice = buf.unfilled();
        let n = match self.project().inner.poll_read(cx, slice)? {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(n) => n,
        };
        buf.advance(n);
        Poll::Ready(Ok(()))
    }
}

impl<T: AsyncWrite> asupersync::io::AsyncWrite for AsupersyncAdapter<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        self.project().inner.poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        self.project().inner.poll_close(cx)
    }
}
