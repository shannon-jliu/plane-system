use std::collections::HashMap;

use serde::Serialize;
use structopt::StructOpt;

use crate::Command;

use super::state::*;

pub type CameraCommand = Command<CameraRequest, CameraResponse>;

#[derive(StructOpt, Debug, Clone)]
pub enum CameraRequest {
    /// view information about the storage media inside of the camera
    Storage(CameraStorageRequest),

    /// view information about the files stored on the camera; download files
    File(CameraFileRequest),

    /// capture an image
    Capture,

    /// power off the camera
    Power(CameraPowerRequest),

    /// disconnect and reconnect to the camera
    Reconnect,

    /// control the camera's zoom lens
    Zoom(CameraZoomRequest),

    /// control the camera's exposure mode
    Exposure(CameraExposureRequest),

    /// control whether the camera saves to its internal storage or to the host
    SaveMode(CameraSaveModeRequest),

    /// control continuous capture
    #[structopt(name = "cc")]
    ContinuousCapture(CameraContinuousCaptureRequest),

    /// perform a usb reset and reconnect
    Reset,
}

#[derive(StructOpt, Debug, Clone)]
pub enum CameraStorageRequest {
    /// list the storage volumes available on the camera
    List,
}

#[derive(StructOpt, Debug, Clone)]
pub enum CameraFileRequest {
    /// list the files available on the camera
    List {
        /// the hexadecimal file handle of a folder; if provided, the contents
        /// of the folder will be listed
        #[structopt(parse(try_from_str = crate::util::parse_hex_u32))]
        parent: Option<u32>,
    },

    /// download a file from the camera
    Get {
        /// the hexadecimal file handle of a file
        #[structopt(parse(try_from_str = crate::util::parse_hex_u32))]
        handle: u32,
    },
}

#[derive(StructOpt, Debug, Clone)]
pub enum CameraExposureRequest {
    Mode(CameraExposureModeRequest),
}

#[derive(StructOpt, Debug, Clone)]
pub enum CameraExposureModeRequest {
    /// get the current exposure mode
    Get,

    /// set the current exposure mode
    Set { mode: CameraExposureMode },
}

impl std::str::FromStr for CameraExposureMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "m" | "manual" | "manual-exposure" => Ok(CameraExposureMode::ManualExposure),
            "p" | "program-auto" => Ok(CameraExposureMode::ProgramAuto),
            "a" | "aperture" | "apeture-priority" => Ok(CameraExposureMode::AperturePriority),
            "s" | "shutter" | "shutter-priority" => Ok(CameraExposureMode::ShutterPriority),
            "i" | "intelligent-auto" => Ok(CameraExposureMode::IntelligentAuto),
            "superior-auto" => Ok(CameraExposureMode::SuperiorAuto),
            "movie-program-auto" => Ok(CameraExposureMode::MovieProgramAuto),
            "movie-aperture-priority" => Ok(CameraExposureMode::MovieAperturePriority),
            "movie-shutter-priority" => Ok(CameraExposureMode::MovieShutterPriority),
            "movie-manual-exposure" => Ok(CameraExposureMode::MovieManualExposure),
            "movie-intelligent-auto" => Ok(CameraExposureMode::MovieIntelligentAuto),
            _ => bail!("invalid camera exposure mode"),
        }
    }
}

#[derive(StructOpt, Debug, Clone)]
pub enum CameraSaveModeRequest {
    /// get the current save mode
    Get,

    /// set the current save mode
    Set { mode: CameraSaveMode },
}

impl std::str::FromStr for CameraSaveMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "host" | "host-device" => Ok(CameraSaveMode::HostDevice),
            "cam" | "camera" => Ok(CameraSaveMode::MemoryCard1),
            _ => bail!("invalid camera save mode"),
        }
    }
}

#[derive(StructOpt, Debug, Clone)]
pub enum CameraZoomRequest {
    Level(CameraZoomLevelRequest),
    Mode(CameraZoomModeRequest),
}

#[derive(StructOpt, Debug, Clone)]
pub enum CameraZoomLevelRequest {
    Get,
    Set { level: u8 },
}

#[derive(StructOpt, Debug, Clone)]
pub enum CameraZoomModeRequest {
    Optical,
    OpticalDigital,
}

#[derive(StructOpt, Debug, Clone)]
pub enum CameraPowerRequest {
    Up,
    Down,
}

#[derive(StructOpt, Debug, Clone)]
pub enum CameraContinuousCaptureRequest {
    Start,
    Stop,
    Interval { interval: f32 },
}

#[derive(Debug, Clone, Serialize)]
pub enum CameraResponse {
    Unit,
    Data {
        data: Vec<u8>,
    },
    File {
        path: std::path::PathBuf,
    },
    StorageInfo {
        storages: HashMap<ptp::StorageId, ptp::PtpStorageInfo>,
    },
    ObjectInfo {
        objects: HashMap<ptp::ObjectHandle, ptp::PtpObjectInfo>,
    },
    ZoomLevel {
        zoom_level: u8,
    },
    SaveMode {
        save_mode: CameraSaveMode,
    },
    ExposureMode {
        exposure_mode: CameraExposureMode,
    },
}
