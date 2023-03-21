use crate::AbciQueryQuery;
use std::net::SocketAddr;
use hbbft::Contribution;
use tokio::sync::mpsc::Receiver;
use tokio::sync::oneshot::Sender as OneShotSender;

// Tendermint Types
// 这里是consensus侧，所以使用的是tendermint abci库
use tendermint_abci::{Client as AbciClient, ClientBuilder};
use tendermint_proto::abci::{
    RequestBeginBlock, RequestDeliverTx, RequestEndBlock, RequestInfo, RequestInitChain,
    RequestQuery, ResponseQuery,
};
use tendermint_proto::types::Header;

// import hbbft library
use hbbft::broadcast::{Broadcast, Message};
use hbbft::{
    crypto::{PublicKey, SecretKey},
    sync_key_gen::{Ack, AckOutcome, Part, PartOutcome, SyncKeyGen},
    Contribution
};

/// The engine drives the ABCI Application by concurrently polling for:
/// 1. Calling the BeginBlock -> DeliverTx -> EndBlock -> Commit event loop on the ABCI App on each Bullshark
///    certificate received. It will also call Info and InitChain to initialize the ABCI App if
///    necessary.
/// 2. Processing Query & Broadcast Tx messages received from the Primary's ABCI Server API and forwarding them to the
///    ABCI App via a Tendermint protobuf client.
pub struct Engine {
    /// The address of the ABCI app
    pub app_address: SocketAddr,
    /// The path to the consensus output of hbbft node, so that the Engine can query it.
    pub store_path: String,
    /// Messages received from the ABCI Server to be forwarded to the engine.
    pub rx_abci_queries: Receiver<(OneShotSender<ResponseQuery>, AbciQueryQuery)>,
    /// The last block height, initialized to the application's latest block by default
    pub last_block_height: i64,
    pub client: AbciClient,
    pub req_client: AbciClient,
}

impl Engine {
    // create a new hbbft engine
    pub fn new(
        app_address: SocketAddr,
        store_path: &str,
        rx_abci_queries: Receiver<(OneShotSender<ResponseQuery>, AbciQueryQuery)>,
    ) -> Self {
        let mut client = ClientBuilder::default().connect(&app_address).unwrap();

        let last_block_height = client
            .info(RequestInfo::default())
            .map(|res| res.last_block_height)
            .unwrap_or_default();

        // Instantiate a new client to not be locked in an Info connection
        let client = ClientBuilder::default().connect(&app_address).unwrap();
        let req_client = ClientBuilder::default().connect(&app_address).unwrap();
        Self {
            app_address,
            store_path: store_path.to_string(),
            rx_abci_queries,
            last_block_height,
            client,
            req_client,
        }
    }

    /// Receives an ordered list of certificates and apply any application-specific logic.
    pub async fn run(&mut self, mut rx_output: Receiver<HBMessage<Contribution>>) -> eyre::Result<()> {
        self.init_chain()?;

        loop {
            tokio::select! {
                // 查询进来的话，就是通过query接口进行处理
                Some((tx, req)) = self.rx_abci_queries.recv() => {
                    self.handle_abci_query(tx, req)?;
                },
                // 有交易传进来，需要进行共识的处理
                Some(contribution) = rx_output.recv() => {
                    self.handle_contribution(contribution)?;
                }
                else => break,
            }
        }

        Ok(())
    }


    /// Handles ABCI queries coming to the hbbft node and forwards them to the ABCI App. Each
    /// handle call comes with a Sender channel which is used to send the response back to the
    /// hbbft node and then to the client.
    ///
    /// Client => hbbft node => handle_abci_query => ABCI App Response=> hbbft node => Client
    fn handle_abci_query(
        &mut self,
        tx: OneShotSender<ResponseQuery>,
        req: AbciQueryQuery,
    ) -> eyre::Result<()> {
        let req_height = req.height.unwrap_or(0);
        let req_prove = req.prove.unwrap_or(false);

        let resp = self.req_client.query(RequestQuery {
            data: req.data.into(),
            path: req.path,
            height: req_height as i64,
            prove: req_prove,
        })?;

        if let Err(err) = tx.send(resp) {
            eyre::bail!("{:?}", err);
        }
        Ok(())
    }


    /// On each new contribution, increment the block height to proposed and run with
    /// BeginBlock -> DeliverTx each tx in the contribution -> EndBlock -> Commit event loop.
    fn handle_contribution(&mut self, contribution: HBMessage<Contribution>) -> eyre::Result<()> {
        // increment block
        let proposed_block_height = self.last_block_height + 1;

        // 如果有交易发生，那么共识模块就会和app模块进行通信：begin block, deliver tx, end block, commit

        // save it for next time
        self.last_block_height = proposed_block_height;
        // drive the app through the event loop
        self.begin_block(proposed_block_height)?;
        // handle contribution/deliver tx
        self.aggregate_contribution_and_deliver_txs(contribution)?;
        self.end_block(proposed_block_height)?;
        self.commit()?;
        Ok(())
    }

    // 
    fn aggregate_contribution_and_deliver_txs() -> eyre::Result<()>{

    }

    fn aggregation_contribution() -> eyre::Result<()>{

    }


    fn deliver_contribution() -> eyre::Result<()>{

    }

}



// imple the ABCI `consensus connection` method: InitChain, BeginBlock, DeliverTx, EndBlock, and Commit
// https://github.com/informalsystems/tendermint-rs/blob/main/abci/src/client.rs
impl Engine {
    /// Calls the `InitChain` hook on the app, ignores "already initialized" errors.
    pub fn init_chain(&mut self) -> eyre::Result<()> {
        let mut client = ClientBuilder::default().connect(&self.app_address)?;
        match client.init_chain(RequestInitChain::default()) {
            Ok(_) => {}
            Err(err) => {
                // ignore errors about the chain being uninitialized
                if err.to_string().contains("already initialized") {
                    log::warn!("{}", err);
                    return Ok(());
                }
                eyre::bail!(err)
            }
        };
        Ok(())
    }

    /// Calls the `BeginBlock` hook on the ABCI app. For now, it just makes a request with
    /// the new block height.
    // If we wanted to, we could add additional arguments to be forwarded from the Consensus
    // to the App logic on the beginning of each block.
    fn begin_block(&mut self, height: i64) -> eyre::Result<()> {
        let req = RequestBeginBlock {
            header: Some(Header {
                height,
                ..Default::default()
            }),
            ..Default::default()
        };

        self.client.begin_block(req)?;
        Ok(())
    }

    /// Calls the `DeliverTx` hook on the ABCI app.
    fn deliver_tx(&mut self, tx: Transaction) -> eyre::Result<()> {
        self.client.deliver_tx(RequestDeliverTx { tx })?;
        Ok(())
    }

    /// Calls the `EndBlock` hook on the ABCI app. For now, it just makes a request with
    /// the proposed block height.
    // If we wanted to, we could add additional arguments to be forwarded from the Consensus
    // to the App logic on the end of each block.
    fn end_block(&mut self, height: i64) -> eyre::Result<()> {
        let req = RequestEndBlock { height };
        self.client.end_block(req)?;
        Ok(())
    }

    /// Calls the `Commit` hook on the ABCI app.
    fn commit(&mut self) -> eyre::Result<()> {
        self.client.commit()?;
        Ok(())
    }
}


pub type Transaction = Vec<u8>;
pub type Contribution = Vec<Transaction>;
#[derive(serde::Deserialize)]
pub enum HBMessage<C: Contribution> {
    HbContribution(C),
}