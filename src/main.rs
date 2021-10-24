use std::{process::exit, str::FromStr, sync::Arc, time::Duration};

use anyhow::Context;
use ctrlc;
use futures::channel::oneshot;
use structopt::StructOpt;
use tokio::{
    spawn,
    sync::{broadcast, watch},
    time::sleep,
};

use gimbal::client::GimbalClient;
use gs::GroundServerClient;
use pixhawk::{client::PixhawkClient, state::PixhawkEvent};
use scheduler::Scheduler;
use state::TelemetryInfo;
use telemetry::TelemetryStream;

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
mod dummy;
mod gimbal;
mod gs;
mod image;
mod pixhawk;
mod scheduler;
mod server;
mod state;
mod telemetry;
mod util;

#[derive(Debug)]
pub struct Channels {
    /// Channel for broadcasting a signal when the system should terminate.
    interrupt: broadcast::Sender<()>,

    /// Channel for broadcasting telemetry information gathered from the gimbal and pixhawk
    telemetry: watch::Receiver<Option<TelemetryInfo>>,

    /// Channel for broadcasting updates to the state of the Pixhawk.
    pixhawk_event: broadcast::Sender<PixhawkEvent>,

    /// Channel for sending instructions to the Pixhawk.
    pixhawk_cmd: flume::Sender<pixhawk::PixhawkCommand>,

    /// Channel for broadcasting updates to the state of the camera.
    camera_event: broadcast::Sender<camera::CameraClientEvent>,

    /// Channel for sending instructions to the camera.
    camera_cmd: flume::Sender<camera::CameraCommand>,

    /// Channel for sending instructions to the gimbal.
    gimbal_cmd: flume::Sender<gimbal::GimbalCommand>,

    /// Channel for sending instructions to the gimbal.
    dummy_cmd: flume::Sender<dummy::DummyCommand>,

    image_event: broadcast::Sender<image::ImageClientEvent>,
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
        let chan = self.channel();

        if chan.is_canceled() {
            panic!("chan is closed");
        }

        chan.send(result)
    }

    fn success(self, data: Res) -> Result<(), Result<Res, Err>> {
        self.channel().send(Ok(data))
    }

    fn error(self, error: Err) -> Result<(), Result<Res, Err>> {
        self.channel().send(Err(error))
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    pretty_env_logger::init_timed();

    let main_args: cli::args::MainArgs = cli::args::MainArgs::from_args();

    let config = if let Some(config_path) = main_args.config {
        debug!("reading config from {:?}", &config_path);
        cli::config::PlaneSystemConfig::read_from_path(config_path)
    } else {
        debug!("reading config from default location");
        cli::config::PlaneSystemConfig::read()
    };

    let config = config.context("failed to read config file")?;

    run_tasks(config).await
}

async fn run_tasks(config: cli::config::PlaneSystemConfig) -> anyhow::Result<()> {
    let (telemetry_sender, telemetry_receiver) = watch::channel(None);
    let (interrupt_sender, _) = broadcast::channel(1);
    let (pixhawk_event_sender, _) = broadcast::channel(64);
    let (camera_event_sender, _) = broadcast::channel(256);
    let (image_event_sender, _) = broadcast::channel(256);
    let (pixhawk_cmd_sender, pixhawk_cmd_receiver) = flume::unbounded();
    let (camera_cmd_sender, camera_cmd_receiver) = flume::unbounded();
    let (gimbal_cmd_sender, gimbal_cmd_receiver) = flume::unbounded();
    let (dummy_cmd_sender, dummy_cmd_receiver) = flume::unbounded();

    let channels = Arc::new(Channels {
        interrupt: interrupt_sender.clone(),
        telemetry: telemetry_receiver,
        pixhawk_event: pixhawk_event_sender,
        pixhawk_cmd: pixhawk_cmd_sender,
        camera_event: camera_event_sender,
        camera_cmd: camera_cmd_sender,
        gimbal_cmd: gimbal_cmd_sender,
        dummy_cmd: dummy_cmd_sender,
        image_event: image_event_sender,
    });

    let mut task_names = Vec::new();
    let mut futures = Vec::new();

    ctrlc::set_handler({
        let interrupt_sender = interrupt_sender.clone();
        move || {
            info!("received interrupt, shutting down");
            let _ = interrupt_sender.send(());
        }
    })
    .expect("could not set ctrl+c handler");

    if config.dummy {
        info!("starting dummy");

        let dummy_task = spawn({
            let mut dummy_client =
                dummy::DummyClient::create(channels.clone(), dummy_cmd_receiver)?;
            async move { dummy_client.run().await }
        });

        task_names.push("dummy");
        futures.push(dummy_task);
    }

    if let Some(pixhawk_config) = config.pixhawk {
        info!("connecting to pixhawk at {}", pixhawk_config.address);
        let pixhawk_task = spawn({
            let mut pixhawk_client = PixhawkClient::connect(
                channels.clone(),
                pixhawk_cmd_receiver,
                pixhawk_config.address,
                pixhawk_config.mavlink,
            )
            .await?;
            async move { pixhawk_client.run().await }
        });

        futures.push(pixhawk_task);
        task_names.push("pixhawk");

        info!("initializing telemetry stream");
        let telemetry_task = spawn({
            let telemetry = TelemetryStream::new(channels.clone(), telemetry_sender);
            async move { telemetry.run().await }
        });

        task_names.push("telemetry");
        futures.push(telemetry_task);
    } else {
        info!("pixhawk address not specified, disabling pixhawk connection and telemetry stream");
    }

    if let Some(camera_config) = config.camera {
        info!("connecting to camera");

        let camera_task = match camera_config.kind {
            cli::config::CameraKind::R10C => {
                trace!("camera kind set to Sony R10C");

                spawn(camera::run(channels.clone(), camera_cmd_receiver))
            }

            // for the future when we switch to a different type of camera
            #[allow(unreachable_patterns)]
            unknown_kind => {
                bail!("camera kind {:?} is not implemented", unknown_kind)
            }
        };

        task_names.push("camera");
        futures.push(camera_task);
    }

    if let Some(image_config) = config.image {
        info!("starting image download task");

        let image_task = spawn(image::run(channels.clone(), image_config));

        task_names.push("image");
        futures.push(image_task);
    }

    if let Some(gimbal_config) = config.gimbal {
        trace!("gimbal kind set to {:?}", gimbal_config.kind);

        info!("initializing gimbal");
        let gimbal_task = spawn({
            let mut gimbal_client = if let Some(gimbal_path) = gimbal_config.device_path {
                GimbalClient::connect_with_path(channels.clone(), gimbal_cmd_receiver, gimbal_path)?
            } else {
                GimbalClient::connect(channels.clone(), gimbal_cmd_receiver, gimbal_config.kind)?
            };

            async move { gimbal_client.run().await }
        });

        task_names.push("gimbal");
        futures.push(gimbal_task);
    }

    if let Some(gs_config) = config.ground_server {
        info!("initializing ground server client");
        let gs_task = spawn({
            let gs_client = GroundServerClient::new(
                channels.clone(),
                reqwest::Url::from_str(&gs_config.address)
                    .context("invalid ground server address")?,
            )?;

            async move { gs_client.run().await }
        });

        task_names.push("ground server client");
        futures.push(gs_task);
    }

    if let Some(scheduler_config) = config.scheduler {
        info!("initializing scheduler");
        let scheduler_task = spawn({
            let mut scheduler = Scheduler::new(channels.clone(), scheduler_config.gps);
            async move { scheduler.run().await }
        });

        task_names.push("scheduler");
        futures.push(scheduler_task);
    }

    info!("initializing plane server");
    let server_address = config
        .plane_server
        .address
        .parse()
        .context("invalid server address")?;

    let server_task = spawn({
        let channels = channels.clone();
        server::serve(channels, server_address)
    });

    task_names.push("server");
    futures.push(server_task);

    info!("intializing cli");
    let cli_task = spawn({
        let channels = channels.clone();
        cli::repl::run(channels)
    });

    task_names.push("cli");
    futures.push(cli_task);

    while futures.len() > 0 {
        // wait for each task to end
        let (result, i, remaining) = futures::future::select_all(futures).await;
        let task_name = task_names.remove(i);

        info!("{} ({}) task ended, {} remaining", task_name, i, task_names.join(", "));

        // if a task ended with an error or did not join properly, end the process
        // with an interrupt
        if let Err(err) = result? {
            error!(
                "got error from {} task, sending interrupt: {:?}",
                task_name, err
            );

            info!("remaining tasks: {:?}", task_names.join(", "));

            let _ = interrupt_sender.send(());

            spawn({
                let task_names = task_names.clone();

                async move {
                    info!("remaining tasks: {:?}", task_names.join(", "));
                    sleep(Duration::from_secs(5)).await;
                    warn!("tasks did not end after 5 seconds, force-quitting");
                    exit(1);
                }
            });
        }

        futures = remaining;
    }

    info!("exit");

    Ok(())
}
