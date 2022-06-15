use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Context;
use futures::{select, FutureExt};
use tokio::{fs::File, io::AsyncWriteExt};

use crate::{
    camera::main::CameraClientEvent, cli::config::ImageConfig, state::Telemetry, Channels,
};

#[derive(Clone, Debug)]
pub struct ImageClientEvent {
    pub data: Arc<Vec<u8>>,
    pub file: PathBuf,
    pub telemetry: Option<Telemetry>,
}

pub async fn run(channels: Arc<Channels>, config: ImageConfig) -> anyhow::Result<()> {
    let mut interrupt_recv = channels.interrupt.subscribe();
    let mut camera_recv = channels.camera_event.subscribe();

    let interrupt_fut = interrupt_recv.recv().fuse();

    futures::pin_mut!(interrupt_fut);

    let mut image_save_dir = config.save_path.clone();
    image_save_dir.push(chrono::Local::now().format("%F_%H-%M-%S").to_string());

    if let Err(err) = tokio::fs::create_dir_all(&image_save_dir).await {
        warn!("could not create image save directory: {}", err);
    }

    loop {
        select! {
            camera_evt = camera_recv.recv().fuse() => {
                if let Ok(camera_evt) = camera_evt {
                    match camera_evt {
                        CameraClientEvent::Download { image_name, image_data, cc_timestamp, .. } => {
                            debug!("image download detected, uploading file to ground server");

                            let telemetry_info = channels.telemetry.borrow().clone();

                            if telemetry_info.is_none() {
                                warn!("no telemetry data available for image capture")
                            }

                            let image_filename = match save(&image_save_dir, &image_name, &image_data, &telemetry_info, cc_timestamp).await {
                                Ok(image_filename) => image_filename,
                                Err(err) => {
                                  warn!("failed to download image: {}", err);
                                  continue;
                                }
                            };

                            let _ = channels.image_event.send(ImageClientEvent {
                              data: image_data,
                              file: image_filename,
                              telemetry: telemetry_info
                            });
                        }
                        _ => {}
                    }
                }
            }
            _ = interrupt_fut => {
                break;
            }
        }
    }

    Ok(())
}

async fn save(
    image_save_dir: impl AsRef<Path>,
    name: &str,
    image: &Vec<u8>,
    telem: &Option<Telemetry>,
    cc_timestamp: Option<chrono::DateTime<chrono::Local>>,
) -> anyhow::Result<PathBuf> {
    let mut image_path = image_save_dir.as_ref().to_owned();
    image_path.push(&name);
    debug!("writing image to file '{}'", image_path.to_string_lossy());

    let mut image_file = File::create(&image_path)
        .await
        .context("failed to create image file")?;

    image_file
        .write_all(&image[..])
        .await
        .context("failed to save image")?;

    info!("wrote image to file '{}'", image_path.to_string_lossy());

    let telem_path = image_path.with_extension("json");

    debug!(
        "writing telemetry to file '{}'",
        telem_path.to_string_lossy()
    );

    let timestamp = cc_timestamp.map(|c| c.to_rfc3339());

    let telem_bytes = serde_json::to_vec(&serde_json::json!({
        "telemetry": telem,
        "cc_timestamp": timestamp,
    }))
    .context("failed to serialize telemetry to JSON")?;

    let mut telem_file = File::create(telem_path)
        .await
        .context("failed to create telemetry file")?;

    telem_file
        .write_all(&telem_bytes[..])
        .await
        .context("failed to write telemetry data to file")?;

    Ok(image_path)
}
