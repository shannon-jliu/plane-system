use crate::{pixhawk::state::PixhawkEvent, state::RegionOfInterest, Channels};
use anyhow::Context;
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;

/// Controls whether the plane is taking pictures of the ground (first-pass),
/// taking pictures of ROIs (second-pass), or doing nothing. Coordinates sending
/// requests to the camera and to the gimbal based on telemetry information
/// received from the Pixhawk.
pub struct Scheduler {
    /// List of regions of interest that should be photographed as soon as
    /// possible. Scheduler will prioritize attempting to photograph nearby ROIs
    /// over increasing ground coverage.
    rois: Vec<RegionOfInterest>,

    /// Channel for receiving from the pixhawk client
    channels: Arc<Channels>,
}

impl Scheduler {
    pub fn new(channels: Arc<Channels>) -> Self {
        Self::with_rois(Vec::new(), channels)
    }

    pub fn with_rois(rois: Vec<RegionOfInterest>, channels: Arc<Channels>) -> Self {
        Self { rois, channels }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let mut pixhawk_recv = self.channels.pixhawk_event.subscribe();
        let mut telemetry_recv = self.channels.telemetry.clone();
        let interrupt_recv = self.channels.interrupt.clone();

        loop {
            if let Ok(Ok(message)) = timeout(Duration::from_millis(10), pixhawk_recv.recv()).await {
                match message {
                    PixhawkEvent::Image {
                        time,
                        foc_len,
                        img_idx,
                        cam_idx,
                        flags,
                        coords,
                        attitude,
                    } => (),
                    _ => (),
                }
            }

            let telemetry = telemetry_recv
                .recv()
                .await
                .context("telemetry stream closed")?;

            debug!("{:?}", telemetry);

            if *interrupt_recv.borrow() {
                break;
            }
        }

        Ok(())
    }
}

pub enum SchedulerEvent {
    ROI(RegionOfInterest),
    Coverage,
}
