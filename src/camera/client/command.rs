use anyhow::Context;
use num_traits::FromPrimitive;
use tokio::sync::{broadcast, RwLock};
use tokio::time::sleep;

use crate::util::retry_async;

use super::util::*;
use super::*;

pub(super) async fn cmd_debug(
    interface: CameraInterfaceRequestBuffer,
    req: CameraCommandDebugRequest,
) -> anyhow::Result<CameraCommandResponse> {
    if let Some(property) = req.property {
        let property_code: CameraPropertyCode =
            FromPrimitive::from_u32(property).context("not a valid camera property code")?;
        println!("dumping {:#X?}", property_code);

        let property = interface
            .enter(|i| async move { i.get_info(property_code).await })
            .await;

        println!("dumping {:#X?}", property);

        if let Some(property) = property {
            if let Some(&value) = req.value_num.first() {
                let property_value = match property.data_type {
                    0x0001 => ptp::PtpData::INT8(value as i8),
                    0x0002 => ptp::PtpData::UINT8(value as u8),
                    0x0003 => ptp::PtpData::INT16(value as i16),
                    0x0004 => ptp::PtpData::UINT16(value as u16),
                    0x0005 => ptp::PtpData::INT32(value as i32),
                    0x0006 => ptp::PtpData::UINT32(value as u32),
                    0x0007 => ptp::PtpData::INT64(value as i64),
                    0x0008 => ptp::PtpData::UINT64(value as u64),
                    _ => bail!("cannot set this property type, not implemented"),
                };

                println!("setting {:#X?} to {:#X?}", property_code, property_value);

                ensure(&interface, property_code, property_value).await?;
            }
        }
    } else {
        warn!("dumping entire state is unimplemented");
    }

    Ok(CameraCommandResponse::Unit)
}

pub(super) async fn cmd_capture(
    interface: CameraInterfaceRequestBuffer,
    ptp_rx: &mut broadcast::Receiver<ptp::PtpEvent>,
) -> anyhow::Result<CameraCommandResponse> {
    ensure_mode(&interface, CameraOperatingMode::StillRec).await?;

    interface
        .enter(|i| async move {
            info!("capturing image");

            debug!("sending half shutter press");

            // press shutter button halfway to fix the focus
            i.control(CameraControlCode::S1Button, ptp::PtpData::UINT16(0x0002))
                .await?;

            debug!("sending full shutter press");

            // shoot!
            i.control(CameraControlCode::S2Button, ptp::PtpData::UINT16(0x0002))
                .await?;

            debug!("sending full shutter release");

            // release
            i.control(CameraControlCode::S2Button, ptp::PtpData::UINT16(0x0001))
                .await?;

            debug!("sending half shutter release");

            // hell yeah
            i.control(CameraControlCode::S1Button, ptp::PtpData::UINT16(0x0001))
                .await?;

            Ok::<_, anyhow::Error>(())
        })
        .await?;

    info!("waiting for image confirmation");

    {
        let watch_fut = watch(&interface, CameraPropertyCode::ShootingFileInfo);
        let wait_fut = wait(ptp_rx, ptp::EventCode::Vendor(0xC204));

        futures::pin_mut!(watch_fut);
        futures::pin_mut!(wait_fut);

        let confirm_fut = futures::future::select(watch_fut, wait_fut);

        let res = tokio::time::timeout(Duration::from_millis(3000), confirm_fut)
            .await
            .context("timed out while waiting for image confirmation")?;

        match res {
            futures::future::Either::Left((watch_res, _)) => {
                watch_res.context("error while waiting for change in shooting file counter")?;
            }
            futures::future::Either::Right((wait_res, _)) => {
                wait_res.context("error while waiting for capture complete event")?;
            }
        }
    }

    Ok(CameraCommandResponse::Unit)
}

pub(super) async fn cmd_continuous_capture(
    interface: CameraInterfaceRequestBuffer,
    req: CameraCommandContinuousCaptureRequest,
) -> anyhow::Result<CameraCommandResponse> {
    match req {
        CameraCommandContinuousCaptureRequest::Start => {
            interface
                .enter(|i| async move {
                    i.control(
                        CameraControlCode::IntervalStillRecording,
                        ptp::PtpData::UINT16(0x0002),
                    )
                    .await
                    .context("failed to start interval recording")
                })
                .await?;
        }
        CameraCommandContinuousCaptureRequest::Stop => {
            interface
                .enter(|i| async move {
                    i.control(
                        CameraControlCode::IntervalStillRecording,
                        ptp::PtpData::UINT16(0x0001),
                    )
                    .await
                    .context("failed to start interval recording")
                })
                .await?;
        }
        CameraCommandContinuousCaptureRequest::Interval { interval } => {
            let interval = (interval * 10.) as u16;

            if interval < 10 {
                bail!("minimum interval is 1 second");
            }

            if interval > 300 {
                bail!("maximum interval is 30 seconds");
            }

            if interval % 5 != 0 {
                bail!("valid intervals are in increments of 0.5 seconds");
            }

            ensure(
                &interface,
                CameraPropertyCode::IntervalTime,
                ptp::PtpData::UINT16(interval),
            )
            .await
            .context("failed to set camera interval")?;
        }
    }

    Ok(CameraCommandResponse::Unit)
}

pub(super) async fn cmd_storage(
    interface: CameraInterfaceRequestBuffer,
    req: CameraCommandStorageRequest,
) -> anyhow::Result<CameraCommandResponse> {
    match req {
        CameraCommandStorageRequest::List => {
            ensure_mode(&interface, CameraOperatingMode::ContentsTransfer).await?;

            debug!("getting storage ids");

            sleep(Duration::from_secs(1)).await;

            debug!("checking for storage ID 0x00010000");

            interface
                .enter(|i| async move {
                    let storage_ids = i.storage_ids().await.context("could not get storage ids")?;

                    if storage_ids.contains(&ptp::StorageId::from(0x00010000)) {
                        bail!("no logical storage available");
                    }

                    debug!("got storage ids: {:?}", storage_ids);

                    let infos: Vec<Result<(_, _), _>> =
                        futures::future::join_all(storage_ids.iter().map(|&id| {
                            let i = &i;
                            async move { i.storage_info(id).await.map(|info| (id, info)) }
                        }))
                        .await;

                    infos
                        .into_iter()
                        .collect::<Result<HashMap<_, _>, _>>()
                        .map(|storages| CameraCommandResponse::StorageInfo { storages })
                })
                .await
        }
    }
}

pub(super) async fn cmd_file(
    interface: CameraInterfaceRequestBuffer,
    req: CameraCommandFileRequest,
    client_tx: broadcast::Sender<CameraClientEvent>,
) -> anyhow::Result<CameraCommandResponse> {
    match req {
        CameraCommandFileRequest::List { parent } => {
            ensure_mode(&interface, CameraOperatingMode::ContentsTransfer).await?;

            debug!("getting object handles");

            interface
                .enter(|i| async move {
                    // wait for storage ID 0x00010001 to exist

                    retry_async(10, Some(Duration::from_secs(1)), || async {
                        debug!("checking for storage ID 0x00010001");

                        let storage_ids =
                            i.storage_ids().await.context("could not get storage ids")?;

                        if !storage_ids.contains(&ptp::StorageId::from(0x00010001)) {
                            bail!("no storage available");
                        } else {
                            Ok(())
                        }
                    })
                    .await?;

                    let object_handles = i
                        .object_handles(
                            ptp::StorageId::from(0x00010001),
                            parent
                                .clone()
                                .map(|v| ptp::ObjectHandle::from(v))
                                .unwrap_or(ptp::ObjectHandle::root()),
                        )
                        .await
                        .context("could not get object handles")?;

                    debug!("got object handles: {:?}", object_handles);

                    futures::future::join_all(object_handles.iter().map(|&id| {
                        let iface = &i;
                        async move { iface.object_info(id).await.map(|info| (id, info)) }
                    }))
                    .await
                    .into_iter()
                    .collect::<Result<HashMap<_, _>, _>>()
                    .map(|objects| CameraCommandResponse::ObjectInfo { objects })
                })
                .await
        }

        CameraCommandFileRequest::Get { handle } => {
            ensure_mode(&interface, CameraOperatingMode::ContentsTransfer).await?;

            let (info, data) = interface
                .enter(|i| async move {
                    let info = i
                        .object_info(ptp::ObjectHandle::from(0xFFFFC001))
                        .await
                        .context("failed to get object info for download")?;

                    let data = i
                        .object_data(ptp::ObjectHandle::from(0xFFFFC001))
                        .await
                        .context("failed to get object data for download")?;

                    Ok::<_, anyhow::Error>((info, data))
                })
                .await
                .context("downloading image data failed")?;

            let _ = client_tx.send(CameraClientEvent::Download {
                image_name: info.filename.clone(),
                image_data: Arc::new(data),
            });

            Ok(CameraCommandResponse::Download {
                name: info.filename,
            })
        }
    }
}

pub(super) async fn cmd_zoom(
    interface: CameraInterfaceRequestBuffer,
    req: CameraCommandZoomRequest,
) -> anyhow::Result<CameraCommandResponse> {
    match req {
        CameraCommandZoomRequest::Level(req) => match req {
            CameraZoomLevelRequest::Set { level } => {
                ensure(
                    &interface,
                    CameraPropertyCode::ZoomAbsolutePosition,
                    ptp::PtpData::UINT16(level as u16),
                )
                .await?;

                return Ok(CameraCommandResponse::ZoomLevel { zoom_level: level });
            }
            CameraZoomLevelRequest::Get => {
                let zoom_value = interface
                    .enter(|i| async move {
                        i.get_value(CameraPropertyCode::ZoomAbsolutePosition)
                            .await
                            .context("failed to query zoom level")
                    })
                    .await?;

                if let ptp::PtpData::UINT16(level) = zoom_value {
                    return Ok(CameraCommandResponse::ZoomLevel {
                        zoom_level: level as u8,
                    });
                }

                bail!("invalid zoom level");
            }
        },
        CameraCommandZoomRequest::Mode(_req) => bail!("unimplemented"),
    }
}

pub(super) async fn cmd_exposure(
    interface: CameraInterfaceRequestBuffer,
    req: CameraCommandExposureRequest,
) -> anyhow::Result<CameraCommandResponse> {
    match req {
        CameraCommandExposureRequest::Mode(req) => match req {
            CameraExposureModeRequest::Set { mode } => {
                ensure(
                    &interface,
                    CameraPropertyCode::ExposureMode,
                    ptp::PtpData::UINT16(mode as u16),
                )
                .await?;

                return Ok(CameraCommandResponse::ExposureMode {
                    exposure_mode: mode,
                });
            }
            CameraExposureModeRequest::Get => {
                let exposure_value = interface
                    .enter(|i| async move {
                        i.get_value(CameraPropertyCode::ExposureMode)
                            .await
                            .context("failed to query exposure mode")
                    })
                    .await?;

                if let ptp::PtpData::UINT16(mode) = exposure_value {
                    if let Some(exposure_mode) = CameraExposureMode::from_u16(mode) {
                        return Ok(CameraCommandResponse::ExposureMode { exposure_mode });
                    }
                }

                bail!("invalid exposure level");
            }
        },
    }
}
