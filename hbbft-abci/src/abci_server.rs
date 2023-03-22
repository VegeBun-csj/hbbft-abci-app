use crate::{AbciQueryQuery, BroadcastTxQuery};

use eyre::WrapErr;
use futures::SinkExt;
use tendermint_proto::abci::ResponseQuery;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot::{channel as oneshot_channel, Sender as OneShotSender};

use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use warp::{Filter, Rejection};

use std::net::SocketAddr;

/// Simple HTTP API server which listens to messages on:
/// * `broadcast_tx`: forwards them to hbbft bind address socket, which will proceed to put
/// it in the consensus process and eventually forward it to the application.
/// * `abci_query`: forwards them over a channel to a handler (typically the application).
pub struct AbciApi<T> {
    // TODO: 内存池，会用到Queueing HoneyBadger
    // mempool_address: SocketAddr,

    // hbbft bind addr(connect one node)
    bind_address: SocketAddr,

    // value
    tx: Sender<(OneShotSender<T>, AbciQueryQuery)>,
}

impl<T: Send + Sync + std::fmt::Debug> AbciApi<T> {
    pub fn new(
        bind_address: SocketAddr,
        tx: Sender<(OneShotSender<T>, AbciQueryQuery)>,
    ) -> Self {
        Self {
            bind_address,
            tx,
        }
    }
}

impl AbciApi<ResponseQuery> {
    pub fn routes(self) -> impl Filter<Extract = impl warp::Reply, Error = Rejection> + Clone {

        // 如果收到的是tx请求
        let route_broadcast_tx = warp::path("broadcast_tx")
            .and(warp::query::<BroadcastTxQuery>())
            .and_then(move |req: BroadcastTxQuery| async move {
                log::warn!("broadcast_tx: {:?}", req);

                // 根据这个地址创建一个连接
                let stream = TcpStream::connect(self.bind_address)
                    .await
                    .wrap_err(format!(
                        "ROUTE_BROADCAST_TX failed to connect to {}",
                        self.bind_address
                    ))
                    .unwrap();
                let mut transport = Framed::new(stream, LengthDelimitedCodec::new());

                if let Err(e) = transport.send(req.tx.clone().into()).await {
                    Ok::<_, Rejection>(format!("ERROR IN: broadcast_tx: {:?}. Err: {}", req, e))
                } else {
                    Ok::<_, Rejection>(format!("broadcast_tx: {:?}", req))
                }
            });

        // 如果收到的是query
        let route_abci_query = warp::path("abci_query")
            .and(warp::query::<AbciQueryQuery>())
            .and_then(move |req: AbciQueryQuery| {
                let tx_abci_queries = self.tx.clone();
                async move {
                    log::warn!("abci_query: {:?}", req);

                    let (tx, rx) = oneshot_channel();
                    match tx_abci_queries.send((tx, req.clone())).await {
                        Ok(_) => {}
                        Err(err) => log::error!("Error forwarding abci query: {}", err),
                    };
                    let resp = rx.await.unwrap();
                    // Return the value
                    Ok::<_, Rejection>(resp.value)
                }
            });

        route_broadcast_tx.or(route_abci_query)
    }
}
