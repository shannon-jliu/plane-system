use anyhow::Context;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::util::run_loop;
use crate::Channels;

use super::interface::*;
use super::*;

pub struct SaveClient {
    iface: SaveInterface,
    channels: Arc<Channels>,
    cmd: flume::Receiver<SaveCommand>,
}

impl SaveClient {
    pub fn connect(
        channels: Arc<Channels>,
        cmd: flume::Receiver<SaveCommand>,
        path: PathBuf,
        cameras: Vec<String>,
    ) -> anyhow::Result<Self> {
        let iface =
            SaveInterface::new(path.clone(), cameras).context("failed to create save interface")?;

        if !path.exists() {
            std::fs::create_dir(path).context("failed to create save directory")?;
        }

        Ok(Self {
            iface,
            channels,
            cmd,
        })
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        trace!("initializing saver");

        let mut interrupt_rx = self.channels.interrupt.subscribe();

        if let Some(Err(err)) = run_loop(
            async {
                loop {
                    if let Ok(cmd) = self.cmd.try_recv() {
                        let result = self.exec(cmd.request()).await;
                        let _ = cmd.respond(result);
                    }

                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            },
            interrupt_rx.recv(),
        )
        .await
        {
            return Err(err);
        }

        Ok(())
    }

    async fn exec(&mut self, cmd: &SaveRequest) -> anyhow::Result<SaveResponse> {
        match cmd {
            SaveRequest::Start {} => self.iface.start_save()?,
            SaveRequest::End {} => self.iface.end_save()?,
        }
        Ok(SaveResponse::Unit)
    }
}