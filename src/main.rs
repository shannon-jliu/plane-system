use std::sync::Arc;

use ctrlc;
use pixhawk::{client::PixhawkClient, state::PixhawkMessage};
use scheduler::Scheduler;
use tokio::{spawn, sync::broadcast};

#[macro_use]
extern crate log;
#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate num_derive;

mod camera;
mod gimbal;
mod gpio;
mod image_upload;
mod pixhawk;
mod scheduler;
mod server;

mod state;

#[derive(Debug)]
pub struct Channels {
    /// Channel for broadcasting a signal when the server should terminate.
    interrupt: broadcast::Sender<()>,

    /// Channel for broadcasting updates to the state of the Pixhawk.
    pixhawk: broadcast::Sender<PixhawkMessage>,
    // camera: Option<broadcast::Receiver<CameraMessage>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();

    let (interrupt_sender, _) = broadcast::channel(1);
    let (pixhawk_sender, _) = broadcast::channel(1024);

    let channels: Arc<Channels> = Arc::new(Channels {
        interrupt: interrupt_sender,
        pixhawk: pixhawk_sender,
    });

    ctrlc::set_handler({
        let channels = channels.clone();

        move || {
            info!("received interrupt, shutting down");
            let _ = channels.interrupt.send(());
        }
    })
    .expect("could not set ctrl+c handler");

    info!("connecting to pixhawk");

    // pixhawk telemetry should be exposed on localhost:5763 for SITL
    // TODO: add case for when it's not the SITL

    let mut pixhawk_client = PixhawkClient::connect(channels.clone(), ":::5763").await?;

    info!("initializing scheduler");

    let mut scheduler = Scheduler::new(channels.clone());

    let pixhawk_task = spawn(async move { pixhawk_client.run().await });
    let server_task = spawn(async move { server::serve(channels.clone()).await });
    let scheduler_task = spawn(async move { scheduler.run().await });

    let futures = vec![pixhawk_task, server_task, scheduler_task];

    let results = futures::future::join_all(futures).await;

    let _ = results.into_iter().map(|res| res?);

    Ok(())
}
