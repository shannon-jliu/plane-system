use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use warp::{self, Filter};
use tokio::sync::broadcast;

use crate::Channels;
use crate::state::RegionOfInterest;

#[derive(Clone)]
struct ServerState {}

enum ServerMessage {
    AddROIs(Vec<RegionOfInterest>),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct AddROIs {
    pub rois: Vec<RegionOfInterest>,
    pub client_type: ClientType,
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ClientType {
    MDLC,
    ADLC,
}

pub async fn serve(channels: Arc<Channels>) -> anyhow::Result<()> {
    info!("initializing server");

    let add_roi = warp::path!("api" / "add-roi")
        .and(warp::post())
        .and(warp::body::json())
        .map(move |body: AddROIs| {
            debug!("received ROIs: {:?}", &body);
            warp::reply()
        });

    let address = ([127, 0, 0, 1], 8080);
    info!("initialized server");
    info!("listening at {:?}", address);
    
    let mut interrupt_recv = channels.interrupt.subscribe();
    let (_, server) = warp::serve(add_roi).bind_with_graceful_shutdown(address, async move {
        interrupt_recv.recv().await.unwrap();
    });

    tokio::spawn(server);

    Ok(())
}
