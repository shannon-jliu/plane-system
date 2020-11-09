use std::{sync::Arc, time::Duration};

use anyhow::Context;
use cli_table::{format::CellFormat, Cell, Row, Table};
use humansize::{file_size_opts, FileSize};
use ptp::PtpData;
use tokio::{
    sync::broadcast::{self, TryRecvError},
    task::spawn_blocking,
    time::delay_for,
};

use crate::{
    cli::repl::{
        CameraCliCommand, CameraFileCliCommand, CameraPowerCliCommand, CameraStorageCliCommand,
        CliCommand, CliResult,
    },
    Channels,
};

use super::{
    interface::CameraInterface, interface::SonyDeviceControlCode,
    interface::SonyDevicePropertyCode, state::CameraMessage,
};

pub struct CameraClient {
    iface: CameraInterface,
    channels: Arc<Channels>,
    cli: broadcast::Receiver<CliCommand>,
    interrupt: broadcast::Receiver<()>,
}

impl CameraClient {
    pub fn connect(channels: Arc<Channels>) -> anyhow::Result<Self> {
        let iface = CameraInterface::new().context("failed to create camera interface")?;

        let cli = channels.cli_cmd.subscribe();
        let interrupt = channels.interrupt.subscribe();

        Ok(CameraClient {
            iface,
            channels,
            cli,
            interrupt,
        })
    }

    pub fn init(&mut self) -> anyhow::Result<()> {
        info!("intializing camera");

        self.iface.connect()?;

        info!("initialized camera");

        Ok(())
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        self.init()?;

        loop {
            match self.interrupt.try_recv() {
                Ok(_) => break,
                Err(TryRecvError::Empty) => {}
                Err(_) => todo!("handle interrupt receiver lagging??"),
            }

            match self.cli.try_recv() {
                Ok(CliCommand::Camera(cmd)) => {
                    let result = self.exec(cmd).await;

                    match result {
                        Ok(result) => self.channels.cli_result.clone().send(result).await?,
                        Err(err) => {
                            error!("{:?}", err);
                            self.channels
                                .cli_result
                                .clone()
                                .send(CliResult::failure())
                                .await?
                        }
                    };
                }
                Ok(CliCommand::Exit) => break,
                _ => {}
            }

            tokio::time::delay_for(Duration::from_secs(1)).await;
        }

        Ok(())
    }

    async fn exec(&mut self, cmd: CameraCliCommand) -> anyhow::Result<CliResult> {
        match cmd {
            CameraCliCommand::Storage(cmd) => match cmd {
                CameraStorageCliCommand::List => {
                    self.ensure_mode(0x04).await?;

                    trace!("getting storage ids");

                    let storage_ids = self
                        .iface
                        .storage_ids()
                        .context("could not get storage ids")?;

                    trace!("got storage ids: {:?}", storage_ids);

                    let storage_infos = storage_ids
                        .iter()
                        .filter(|id| **id != 0xFFFFFFFF.into() && **id != 0x00010000.into())
                        .map(|&id| (id, self.iface.storage_info(id)))
                        .collect::<Vec<_>>();

                    trace!("got storage infos: {:?}", storage_infos);

                    let title_row = Row::new(vec![
                        Cell::new("id", Default::default()),
                        Cell::new("label", Default::default()),
                        Cell::new("capacity", Default::default()),
                        Cell::new("space", Default::default()),
                        Cell::new("access", Default::default()),
                        Cell::new("description", Default::default()),
                    ]);

                    let mut table_rows = vec![title_row];

                    table_rows.extend(storage_infos.into_iter().map(|(id, result)| match result {
                        Ok(info) => {
                            let max_capacity_str =
                                info.max_capacity.file_size(file_size_opts::BINARY).unwrap();

                            let free_space_str = info
                                .free_space_in_bytes
                                .file_size(file_size_opts::BINARY)
                                .unwrap();

                            let access_str = match info.access_capability {
                                ptp::AccessType::Standard(cap) => {
                                    let access_str = match cap {
                                        ptp::StandardAccessType::ReadWrite => "r+w",
                                        ptp::StandardAccessType::ReadOnlyNoDelete => "r",
                                        ptp::StandardAccessType::ReadOnly => "r+d",
                                    };

                                    access_str.to_owned()
                                }
                                ptp::AccessType::Reserved(val) => format!("0x{:04x}", val),
                            };

                            Row::new(vec![
                                Cell::new(&format!("{}", id), Default::default()),
                                Cell::new(&info.volume_label, Default::default()),
                                Cell::new(&max_capacity_str, Default::default()),
                                Cell::new(&free_space_str, Default::default()),
                                Cell::new(&access_str, Default::default()),
                                Cell::new(&info.storage_description, Default::default()),
                            ])
                        }
                        Err(_) => Row::new(vec![
                            Cell::new(&format!("{}", id), Default::default()),
                            Cell::new("error", Default::default()),
                            Cell::new("error", Default::default()),
                            Cell::new("error", Default::default()),
                            Cell::new("error", Default::default()),
                            Cell::new("error", Default::default()),
                        ]),
                    }));

                    let table = Table::new(table_rows, cli_table::format::NO_BORDER_COLUMN_TITLE)
                        .context("could not create table")?;

                    table.print_stdout().context("could not write table")?;

                    Ok(CliResult::success())
                }
            },
            CameraCliCommand::File(cmd) => match cmd {
                CameraFileCliCommand::List => {
                    self.ensure_mode(0x04).await?;

                    trace!("getting object handles");

                    let object_handles = self
                        .iface
                        .object_handles(
                            ptp::StorageId::from(0x00010001),
                            Some(ptp::ObjectHandle::root()),
                        )
                        .context("could not get object handles")?;

                    trace!("got object handles: {:?}", object_handles);

                    let object_infos = object_handles
                        .iter()
                        .map(|&id| (id, self.iface.object_info(id)))
                        .collect::<Vec<_>>();

                    trace!("got object infos: {:?}", object_infos);

                    let title_row = Row::new(vec![
                        Cell::new("handle", Default::default()),
                        Cell::new("filename", Default::default()),
                        Cell::new("type", Default::default()),
                        Cell::new("compressed size", Default::default()),
                    ]);

                    let mut table_rows = vec![title_row];

                    table_rows.extend(object_infos.into_iter().map(
                        |(handle, result)| match result {
                            Ok(info) => {
                                let compressed_size_str = info
                                    .object_compressed_size
                                    .file_size(file_size_opts::BINARY)
                                    .unwrap();

                                Row::new(vec![
                                    Cell::new(&format!("{}", handle), Default::default()),
                                    Cell::new(&format!("{}", info.filename), Default::default()),
                                    Cell::new(&format!("{:?}", info.object_format), Default::default()),
                                    Cell::new(&compressed_size_str, Default::default()),
                                ])
                            }
                            Err(_) => Row::new(vec![
                                Cell::new(&format!("{}", handle), Default::default()),
                                Cell::new("error", Default::default()),
                                Cell::new("error", Default::default()),
                                Cell::new("error", Default::default()),
                            ]),
                        },
                    ));

                    let table = Table::new(table_rows, cli_table::format::NO_BORDER_COLUMN_TITLE)
                        .context("could not create table")?;

                    table.print_stdout().context("could not write table")?;

                    Ok(CliResult::success())
                }
            },
            CameraCliCommand::Power(cmd) => {
                self.ensure_mode(0x02).await?;

                match cmd {
                    CameraPowerCliCommand::Up => self
                        .iface
                        .execute(SonyDeviceControlCode::PowerOff, ptp::PtpData::UINT16(0))?,
                    CameraPowerCliCommand::Down => self
                        .iface
                        .execute(SonyDeviceControlCode::PowerOff, ptp::PtpData::UINT16(1))?,
                };

                Ok(CliResult::success())
            }
            _ => todo!(),
        }
    }

    async fn ensure_mode(&mut self, mode: u8) -> anyhow::Result<()> {
        let mut retries = 0usize;

        trace!("checking operating mode");

        while retries < 10 {
            let current_state = self
                .iface
                .update()
                .context("could not get current camera state")?;

            let current_op_mode = current_state.get(&SonyDevicePropertyCode::OperatingMode);

            debug!("current op mode: {:?}", current_op_mode);

            if let Some(PtpData::UINT8(current_op_mode)) = current_op_mode.map(|d| &d.current) {
                if *current_op_mode == mode {
                    // we are in the right mode, break
                    break;
                }
            }

            debug!("setting operating mode to {:04x}", mode);

            self.iface
                .set(SonyDevicePropertyCode::OperatingMode, PtpData::UINT8(mode))
                .context("failed to set operating mode of camera")?;

            delay_for(Duration::from_millis(1000)).await;

            retries += 1;
        }

        delay_for(Duration::from_millis(1000)).await;

        Ok(())
    }
}
