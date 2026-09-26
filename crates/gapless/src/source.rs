use std::pin::Pin;
use std::time::Duration;

use futures::future::BoxFuture;
use futures::{Stream, StreamExt};
use solami::geyser::geyser_client::GeyserClient;
use solami::geyser::subscribe_update::UpdateOneof;
use solami::geyser::{
    CommitmentLevel, GetSlotRequest, SubscribeReplayInfoRequest, SubscribeRequest,
    SubscribeRequestPing, SubscribeUpdate,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Status;
use tonic::codec::CompressionEncoding;
use tonic::metadata::AsciiMetadataValue;
use tonic::transport::{Channel, ClientTlsConfig};

use crate::Error;

pub type UpdateStream = Pin<Box<dyn Stream<Item = Result<SubscribeUpdate, Status>> + Send>>;

/// Where subscriptions come from. [`SolamiSource`] talks to Solami; tests and offline mode
/// plug in their own.
pub trait Source: Send + Sync + 'static {
    /// Open a subscription. A request with `from_slot` set replays from that slot first.
    fn subscribe(
        &self,
        request: SubscribeRequest,
    ) -> BoxFuture<'static, Result<UpdateStream, Status>>;

    /// The chain tip at `processed`.
    fn tip(&self) -> BoxFuture<'static, Result<u64, Status>>;

    /// The oldest slot a replay can start from, if the server says.
    fn first_available(&self) -> BoxFuture<'static, Result<Option<u64>, Status>>;
}

/// Solami's Yellowstone gRPC endpoint, through the Solami SDK's generated Geyser client.
#[derive(Clone)]
pub struct SolamiSource {
    client: GeyserClient<Channel>,
    token: AsciiMetadataValue,
}

impl SolamiSource {
    pub fn new(grpc_url: &str, api_key: &str, compression: bool) -> Result<Self, Error> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut endpoint = Channel::from_shared(grpc_url.to_owned())
            .map_err(|e| Error::Config(format!("gRPC URL: {e}")))?
            .connect_timeout(Duration::from_secs(10))
            .http2_keep_alive_interval(Duration::from_secs(10))
            .keep_alive_timeout(Duration::from_secs(20))
            .keep_alive_while_idle(true);
        if grpc_url.starts_with("https") {
            endpoint = endpoint.tls_config(ClientTlsConfig::new().with_native_roots())?;
        }
        let mut client =
            GeyserClient::new(endpoint.connect_lazy()).max_decoding_message_size(64 * 1024 * 1024);
        if compression {
            client = client.accept_compressed(CompressionEncoding::Zstd);
        }
        let token = api_key
            .parse()
            .map_err(|_| Error::Config("the API key is not a valid header value".into()))?;
        Ok(Self { client, token })
    }

    fn authed<T>(&self, body: T) -> tonic::Request<T> {
        let mut req = tonic::Request::new(body);
        req.metadata_mut().insert("x-token", self.token.clone());
        req
    }
}

impl Source for SolamiSource {
    fn subscribe(
        &self,
        request: SubscribeRequest,
    ) -> BoxFuture<'static, Result<UpdateStream, Status>> {
        let mut client = self.client.clone();
        let this = self.clone();
        Box::pin(async move {
            let (sink, rx) = mpsc::channel(8);
            sink.send(request)
                .await
                .map_err(|_| Status::internal("request channel closed"))?;
            let updates = client
                .subscribe(this.authed(ReceiverStream::new(rx)))
                .await?
                .into_inner();
            // Answer pings so load balancers keep the stream open. The closure owns `sink`,
            // which also keeps our side of the request stream open.
            let stream = updates.map(move |item| {
                if let Ok(update) = &item
                    && matches!(update.update_oneof, Some(UpdateOneof::Ping(_)))
                {
                    let _ = sink.try_send(SubscribeRequest {
                        ping: Some(SubscribeRequestPing { id: 1 }),
                        ..Default::default()
                    });
                }
                item
            });
            Ok(Box::pin(stream) as UpdateStream)
        })
    }

    fn tip(&self) -> BoxFuture<'static, Result<u64, Status>> {
        let mut client = self.client.clone();
        let req = self.authed(GetSlotRequest {
            commitment: Some(CommitmentLevel::Processed as i32),
        });
        Box::pin(async move { Ok(client.get_slot(req).await?.into_inner().slot) })
    }

    fn first_available(&self) -> BoxFuture<'static, Result<Option<u64>, Status>> {
        let mut client = self.client.clone();
        let req = self.authed(SubscribeReplayInfoRequest {});
        Box::pin(async move {
            Ok(client
                .subscribe_replay_info(req)
                .await?
                .into_inner()
                .first_available)
        })
    }
}
