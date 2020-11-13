use std::sync::Arc;

use anyhow::Context;
use camera::{client::CameraClient, state::CameraEvent};
use ctrlc;
use pixhawk::{client::PixhawkClient, state::PixhawkEvent};
use scheduler::Scheduler;
use state::TelemetryInfo;
use structopt::StructOpt;
use telemetry::TelemetryStream;
use tokio::{spawn, sync::*};

#[macro_use]
extern crate log;
#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate num_derive;
#[macro_use]
extern crate async_trait;

mod camera;
mod cli;
mod gimbal;
mod pixhawk;
mod scheduler;
mod server;
mod state;
mod telemetry;
mod util;

#[derive(Debug)]
pub struct Channels {
    /// Channel for broadcasting a signal when the server should terminate.
    interrupt: watch::Receiver<bool>,

    /// Channel for broadcasting telemetry information gathered from the gimbal and pixhawk
    telemetry: watch::Receiver<Option<TelemetryInfo>>,

    /// Channel for broadcasting updates to the state of the Pixhawk.
    pixhawk_event: broadcast::Sender<PixhawkEvent>,

    /// Channel for sending instructions to the Pixhawk.
    pixhawk_cmd: mpsc::Sender<pixhawk::PixhawkCommand>,

    /// Channel for broadcasting updates to the state of the camera.
    camera_event: broadcast::Sender<CameraEvent>,

    /// Channel for sending instructions to the camera.
    camera_cmd: mpsc::Sender<camera::CameraCommand>,
}

#[derive(Debug)]
pub struct Command<Req, Res, Err = anyhow::Error> {
    request: Req,
    chan: oneshot::Sender<Result<Res, Err>>,
}

impl<Req, Res, Err> Command<Req, Res, Err> {
    fn new(request: Req) -> (Self, oneshot::Receiver<Result<Res, Err>>) {
        let (sender, receiver) = oneshot::channel();

        let cmd = Command {
            chan: sender,
            request,
        };

        (cmd, receiver)
    }

    fn channel(self) -> oneshot::Sender<Result<Res, Err>> {
        self.chan
    }

    fn request(&self) -> &Req {
        &self.request
    }

    fn respond(self, result: Result<Res, Err>) -> Result<(), Result<Res, Err>> {
        self.channel().send(result)
    }

    fn success(self, data: Res) -> Result<(), Result<Res, Err>> {
        self.channel().send(Ok(data))
    }

    fn error(self, error: Err) -> Result<(), Result<Res, Err>> {
        self.channel().send(Err(error))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();

    let main_args: cli::args::MainArgs = cli::args::MainArgs::from_args();

    let config = if let Some(config_path) = main_args.config {
        debug!("reading config from {:?}", &config_path);
        cli::config::PlaneSystemConfig::read_from_path(config_path)
    } else {
        debug!("reading config from default location");
        cli::config::PlaneSystemConfig::read()
    };

    let config = config.context("failed to read config file")?;

    let (interrupt_sender, interrupt_receiver) = watch::channel(false);
    let (telemetry_sender, telemetry_receiver) = watch::channel(None);
    let (pixhawk_event_sender, _) = broadcast::channel(64);
    let (pixhawk_cmd_sender, pixhawk_cmd_receiver) = mpsc::channel(64);
    let (camera_event_sender, _) = broadcast::channel(256);
    let (camera_cmd_sender, camera_cmd_receiver) = mpsc::channel(256);

    let channels = Arc::new(Channels {
        interrupt: interrupt_receiver,
        telemetry: telemetry_receiver,
        pixhawk_event: pixhawk_event_sender,
        pixhawk_cmd: pixhawk_cmd_sender,
        camera_event: camera_event_sender,
        camera_cmd: camera_cmd_sender,
    });

    let mut futures = Vec::new();

    ctrlc::set_handler({
        let channels = channels.clone();

        move || {
            info!("received interrupt, shutting down");
            let _ = interrupt_sender.broadcast(true);
        }
    })
    .expect("could not set ctrl+c handler");

    if let Some(pixhawk_address) = config.pixhawk.address {
        info!("connecting to pixhawk at {}", pixhawk_address);

        let mut pixhawk_client =
            PixhawkClient::connect(channels.clone(), pixhawk_cmd_receiver, pixhawk_address).await?;
        let pixhawk_task = spawn(async move { pixhawk_client.run().await });

        futures.push(pixhawk_task);

        info!("initializing telemetry stream");
        let mut telemetry = TelemetryStream::new(channels.clone(), telemetry_sender);
        let telemetry_task = spawn(async move { telemetry.run().await });
        futures.push(telemetry_task);
    } else {
        info!("pixhawk address not specified, disabling pixhawk connection and telemetry stream");
    }

    if config.camera {
        info!("connecting to camera");
        let camera_task = spawn({
            let mut camera_client = CameraClient::connect(channels.clone(), camera_cmd_receiver)?;
            async move { camera_client.run().await }
        });
        futures.push(camera_task);
    }

    info!("initializing scheduler");
    let scheduler_task = spawn({
        let scheduler = Scheduler::new(channels.clone());
        async move { scheduler.run().await }
    });
    futures.push(scheduler_task);

    info!("initializing server");
    let server_address = config
        .server
        .address
        .parse()
        .context("invalid server address")?;
    let server_task = spawn({
        let channels = channels.clone();
        server::serve(channels, server_address)
    });
    futures.push(server_task);

    info!("intializing cli");
    let cli_task = spawn({
        let channels = channels.clone();
        cli::repl::run(channels)
    });
    futures.push(cli_task);

    // wait for any of these tasks to end
    let (result, _, _) = futures::future::select_all(futures).await;
    result?
}
