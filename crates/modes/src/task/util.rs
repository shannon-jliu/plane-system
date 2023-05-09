//utility file to abstract out repetitive tasks
//include code for when getting within certain distance
use ps_main_camera::CameraRequest;
//use ps_telemetry::PixhawkTelemetry;
use geo::algorithm::euclidean_distance::EuclideanDistance;
use ps_gimbal::GimbalRequest;
use ps_gimbal::GimbalResponse;
use ps_main_camera::CameraResponse;
use ps_telemetry::Telemetry;
use thiserror::Error;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio::time::sleep;
use tokio::time::Duration;

#[derive(Error, Debug)]
pub enum ParseTelemetryError {
    #[error("invalid latitude given")]
    InvalidLat,
    #[error("missing longitude given")]
    InvalidLon,
}

// if enter is true, sleeps until the telemetry enters in range of a waypoint
// if enter is false, sleeps until the telemetry exits the range of a waypoint
pub async fn transition_by_distance(
    waypoints: &[geo::Point],
    telemetry_rx: watch::Receiver<Telemetry>,
    distance_threshold: u64,
    enter: bool,
) -> Result<(), ParseTelemetryError> {
    //"grace period" for cc command action
    let distance = if enter {
        distance_threshold + 1
    } else {
        distance_threshold - 1
    };
    let wait_to_check = 250;
    loop {
        match in_range(waypoints, telemetry_rx.clone(), distance_threshold) {
            Ok(in_range) => {
                if in_range != enter {
                    sleep(Duration::from_millis(wait_to_check)).await;
                } else {
                    break;
                }
            }
            Err(e) => return Err(e),
        }
    }
    return Ok(());
}

// parses telemetry for distance search
fn get_telemetry(
    telemetry_rx: watch::Receiver<Telemetry>,
) -> Result<geo::Point, ParseTelemetryError> {
    let telemetry = telemetry_rx.borrow();
    let mut lon_float: f64 = 0.0;
    let mut lat_float: f64 = 0.0;
    let lon_float_opt = telemetry.pixhawk.as_ref().map(|p| p.position.0.point.x());
    match lon_float_opt {
        Some(lon) => {
            lon_float = lon as f64;
        }
        None => return Err(ParseTelemetryError::InvalidLon),
    }
    let lat_float_opt = telemetry.pixhawk.as_ref().map(|p| p.position.0.point.y());
    match lat_float_opt {
        Some(lat) => {
            lat_float = lat as f64;
        }
        None => return Err(ParseTelemetryError::InvalidLat),
    }
    Ok(geo::Point::new(lon_float, lat_float))
}

// checks whether the telemetry is in a given range of any waypoints
fn in_range(
    waypoint: &[geo::Point],
    telemetry_rx: watch::Receiver<Telemetry>,
    distance_threshold: u64,
) -> Result<bool, ParseTelemetryError> {
    let mut distance = 0.0;
    let telemetry_point_result = get_telemetry(telemetry_rx.clone());
    match telemetry_point_result {
        Ok(telemetry_point) => {
            for point in waypoint {
                distance += telemetry_point.euclidean_distance(point) / (waypoint.len() as f64);
            }
            return Ok(distance as u64 <= distance_threshold);
        }
        Err(e) => return Err(e), //is this losing information by doing this?
    }
}

// starts continous capture
pub async fn start_cc(
    main_camera_tx: flume::Sender<(
        CameraRequest,
        tokio::sync::oneshot::Sender<Result<CameraResponse, anyhow::Error>>,
    )>,
) -> Result<CameraResponse, anyhow::Error> {
    command_camera(
        main_camera_tx,
        CameraRequest::ContinuousCapture(ps_main_camera::CameraContinuousCaptureRequest::Start),
    )
    .await
}

// requests gimbal rotation by given roll, pitch values in degrees
pub async fn rotate_gimbal(
    roll: f64,
    pitch: f64,
    gimbal_tx: flume::Sender<(
        GimbalRequest,
        tokio::sync::oneshot::Sender<Result<GimbalResponse, anyhow::Error>>,
    )>,
) -> Result<GimbalResponse, anyhow::Error> {
    let request = GimbalRequest::Control { roll, pitch };
    command_gimbal(gimbal_tx, request).await
}

// ends continous capture
pub async fn end_cc(
    main_camera_tx: flume::Sender<(
        CameraRequest,
        tokio::sync::oneshot::Sender<Result<CameraResponse, anyhow::Error>>,
    )>,
) -> Result<CameraResponse, anyhow::Error> {
    command_camera(
        main_camera_tx,
        CameraRequest::ContinuousCapture(ps_main_camera::CameraContinuousCaptureRequest::Stop),
    )
    .await
}

// sends a request to the main camera to take a single image
pub async fn capture(
    main_camera_tx: flume::Sender<(
        CameraRequest,
        tokio::sync::oneshot::Sender<Result<CameraResponse, anyhow::Error>>,
    )>,
) -> Result<CameraResponse, anyhow::Error> {
    let request = CameraRequest::Capture {
        burst_duration: None,
        burst_high_speed: false,
    };
    command_camera(main_camera_tx, request).await
}

// sends a given request to the camera
async fn command_camera(
    main_camera_tx: flume::Sender<(
        CameraRequest,
        tokio::sync::oneshot::Sender<Result<CameraResponse, anyhow::Error>>,
    )>,
    request: CameraRequest,
) -> Result<CameraResponse, anyhow::Error> {
    let (tx, rx) = oneshot::channel();
    if let Err(_) = main_camera_tx.send_async((request, tx)).await {
        anyhow::bail!("could not send command");
    }
    rx.await?
}

// sends a given request to the gimbal, waits until request is finished
async fn command_gimbal(
    gimbal_tx: flume::Sender<(
        GimbalRequest,
        tokio::sync::oneshot::Sender<Result<GimbalResponse, anyhow::Error>>,
    )>,
    request: GimbalRequest,
) -> Result<GimbalResponse, anyhow::Error> {
    let (tx, rx) = oneshot::channel();
    if let Err(_) = gimbal_tx.send_async((request, tx)).await {
        anyhow::bail!("could not send command");
    }
    sleep(Duration::from_millis(3)).await;
    rx.await?
    //TODO: figure out how to determine whether the gimbal is at the correct angle
    //maybe for ex. check from telemetry if the current angle is correct, add gimbal angle to telemetry
}