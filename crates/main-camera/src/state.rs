use std::{fmt::Display, str::FromStr, sync::Arc};

use anyhow::{bail, Context};
use num_traits::{FromPrimitive, ToPrimitive};
use serde::Serialize;

#[derive(Debug, Clone)]
pub enum CameraEvent {
    Capture {
        timestamp: chrono::DateTime<chrono::Local>,
    },
    Download {
        image_name: String,
        image_data: Arc<Vec<u8>>,
        /// The timestamp of this image, if it was received asynchronously via
        /// continuous capture.
        cc_timestamp: Option<chrono::DateTime<chrono::Local>>,
    },
    Error(ErrorMode),
}

#[repr(u16)]
#[derive(Debug, Copy, Clone, FromPrimitive, ToPrimitive, Serialize, Eq, PartialEq)]
pub enum ExposureMode {
    ManualExposure = 0x0001,
    ProgramAuto,
    AperturePriority,
    ShutterPriority,
    IntelligentAuto = 0x8000,
    SuperiorAuto,
    MovieProgramAuto = 0x8050,
    MovieAperturePriority,
    MovieShutterPriority,
    MovieManualExposure,
    MovieIntelligentAuto,
}

#[repr(u16)]
#[derive(Debug, Copy, Clone, FromPrimitive, ToPrimitive, Serialize, Eq, PartialEq)]
pub enum FocusMode {
    Manual = 0x0001,
    AutoFocusStill = 0x0002,
    AutoFocusContinuous = 0x8004,
}

impl Display for FocusMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FocusMode::Manual => write!(f, "Manual"),
            FocusMode::AutoFocusStill => write!(f, "Auto (Still)"),
            FocusMode::AutoFocusContinuous => write!(f, "Auto (Continuous)"),
        }
    }
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, FromPrimitive, ToPrimitive, Serialize, Eq, PartialEq)]
pub enum FocusIndication {
    AFUnlock = 0x01,
    AFLock = 0x02,
    /// AF Lock warning (unable to range)
    AFWarning = 0x03,
    Focusing = 0x05,
    /// Focused in continuous focus mode
    FocusedContinuous = 0x06,
}

impl Display for FocusIndication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FocusIndication::AFUnlock => write!(f, "AF Unlock"),
            FocusIndication::AFLock => write!(f, "AF Lock"),
            FocusIndication::AFWarning => write!(f, "AF Warning"),
            FocusIndication::Focusing => write!(f, "Focusing"),
            FocusIndication::FocusedContinuous => write!(f, "AF Lock (Continuous)"),
        }
    }
}

#[repr(u16)]
#[derive(Debug, Copy, Clone, FromPrimitive, ToPrimitive, Serialize, Eq, PartialEq)]
pub enum DriveMode {
    Normal = 0x0001,
    SelfTimer10 = 0x8004,
    SelfTimer2 = 0x8005,
    ContinuousShot = 0x8013,
    SpeedPriorityContinuousShot = 0x8014,
}

impl Display for DriveMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DriveMode::Normal => write!(f, "Normal"),
            DriveMode::SelfTimer10 => write!(f, "Self Timer 10s"),
            DriveMode::SelfTimer2 => write!(f, "Self Timer 2s"),
            DriveMode::ContinuousShot => write!(f, "Continuous Shot"),
            DriveMode::SpeedPriorityContinuousShot => write!(f, "Speed Priority Continuous Shot"),
        }
    }
}

#[repr(u16)]
#[derive(Debug, Copy, Clone, FromPrimitive, ToPrimitive, Serialize, Eq, PartialEq)]
pub enum ZoomMode {
    Optical,
    OpticalDigital,
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, FromPrimitive, ToPrimitive, Serialize, Eq, PartialEq)]
pub enum CompressionMode {
    Std = 0x02,
    Fine = 0x03,
    RawJpeg = 0x13,
}

#[repr(u16)]
#[derive(Debug, Copy, Clone, FromPrimitive, ToPrimitive, Serialize, Eq, PartialEq)]
pub enum SaveMedia {
    HostDevice = 0x0001,
    MemoryCard1 = 0x0002,
}

#[derive(Debug, Copy, Clone, Serialize, Eq, PartialEq)]
pub enum ShutterSpeed {
    /// Bulb
    Bulb,
    Fraction {
        numerator: u16,
        denominator: u16,
    },
}

impl FromPrimitive for ShutterSpeed {
    fn from_i64(_n: i64) -> Option<Self> {
        None
    }

    fn from_u64(n: u64) -> Option<Self> {
        if n == 0xFFFF_FFFE {
            Some(ShutterSpeed::Bulb)
        } else {
            Some(ShutterSpeed::Fraction {
                numerator: ((n >> 16) & 0xFFFF) as u16,
                denominator: (n & 0xFFFF) as u16,
            })
        }
    }
}

impl ToPrimitive for ShutterSpeed {
    fn to_i64(&self) -> Option<i64> {
        None
    }

    fn to_u64(&self) -> Option<u64> {
        match *self {
            ShutterSpeed::Bulb => Some(0xFFFF_FFFE),
            ShutterSpeed::Fraction {
                numerator,
                denominator,
            } => Some((numerator as u64) << 16 | (denominator as u64)),
        }
    }
}

impl Display for ShutterSpeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Bulb => write!(f, "BULB"),
            Self::Fraction {
                numerator,
                denominator,
            } => {
                if numerator > denominator {
                    if numerator % denominator == 0 {
                        write!(f, "{}\"", numerator / denominator)
                    } else {
                        write!(f, "{:.1}\"", numerator as f32 / denominator as f32)
                    }
                } else {
                    write!(f, "{}/{}", numerator, denominator)
                }
            }
        }
    }
}

impl FromStr for ShutterSpeed {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.to_ascii_uppercase() == "BULB" {
            return Ok(Self::Bulb);
        }

        if let Ok(f) = f32::from_str(s) {
            if f <= 0. {
                bail!("shutter speed must be positive");
            }

            return Ok(Self::Fraction {
                numerator: (f * 10.) as u16,
                denominator: 10,
            });
        }

        if let Some((num, den)) = s.split_once('/') {
            if let (Ok(numerator), Ok(denominator)) = (u16::from_str(num), u16::from_str(den)) {
                return Ok(Self::Fraction {
                    numerator,
                    denominator,
                });
            }
        }

        bail!("shutter speed must be 'BULB', a decimal, or a fraction")
    }
}

#[derive(Debug, Copy, Clone, Serialize, Eq, PartialEq)]
pub enum Iso {
    Auto,
    Value(u16),
}

impl FromPrimitive for Iso {
    fn from_i64(_n: i64) -> Option<Self> {
        None
    }

    fn from_u64(n: u64) -> Option<Self> {
        if n == 0x00FF_FFFF {
            Some(Self::Auto)
        } else {
            Some(Self::Value((n & 0xFFFF) as u16))
        }
    }
}

impl ToPrimitive for Iso {
    fn to_i64(&self) -> Option<i64> {
        None
    }

    fn to_u64(&self) -> Option<u64> {
        match self {
            Self::Auto => Some(0x00FF_FFFF),
            Self::Value(n) => Some(*n as u64),
        }
    }
}

impl Display for Iso {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => write!(f, "ISO Auto"),
            Self::Value(v) => write!(f, "ISO {}", v),
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Eq, PartialEq)]
pub enum Aperture {
    Undefined,
    Value(u16),
}

impl FromPrimitive for Aperture {
    fn from_i64(_: i64) -> Option<Self> {
        None
    }

    fn from_u64(n: u64) -> Option<Self> {
        if n == 0xFFFE {
            Some(Self::Undefined)
        } else {
            Some(Self::Value((n & 0xFFFF) as u16))
        }
    }
}

impl ToPrimitive for Aperture {
    fn to_i64(&self) -> Option<i64> {
        None
    }

    fn to_u64(&self) -> Option<u64> {
        match self {
            Self::Undefined => Some(0xFFFE),
            Self::Value(n) => Some(*n as u64),
        }
    }
}

impl Display for Aperture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Undefined => write!(f, "F??.?"),
            Self::Value(v) => write!(f, "F{:.1}", v as f32 / 100.),
        }
    }
}

impl FromStr for Aperture {
    type Err = anyhow::Error;

    fn from_str(mut s: &str) -> Result<Self, Self::Err> {
        if s.starts_with('F') || s.starts_with('f') {
            s = &s[1..];
        }

        let f =
            f32::from_str(s).context("aperture must be a decimal, optionally prefixed with F")?;

        Ok(Self::Value((f * 100.) as u16))
    }
}

#[repr(u16)]
#[derive(Debug, Copy, Clone, FromPrimitive, ToPrimitive, Serialize, Eq, PartialEq)]
pub enum ErrorMode {
    /// Hardware failure, etc
    Fatal = 0x8000,

    /// Error of recording still imageand movie, etc
    RecordingFailed = 0x4000,

    /// Full of still image, movie, etc.
    RecordingFailedStorageFull = 0x2000,

    /// Full of memory card, etc.
    RecordingFailedMediaFull = 0x1000,

    /// Data error, access error of memory card, etc.
    Media = 0x0800,

    /// Unsupported memory card, etc.
    UnsupportedMedia = 0x0400,

    /// Error of unsupported imagesize, etc.
    IncompatibleMedia = 0x0200,

    /// Media none
    NoMedia = 0x0100,

    /// During the recovery of media
    MediaInRecovery = 0x0080,

    MediaRecoveryFailed = 0x0040,

    Temperature = 0x0020,

    Battery = 0x0010,

    Reserved = 0x0008,

    LensNotRecognized = 0x0004,

    CaptureOnCapturing = 0x0002,

    SettingFailure = 0x0001,

    None = 0x0000,
}