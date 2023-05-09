use clap::Subcommand;
use std::num::ParseFloatError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParsePointError {
    #[error("invalid coordinates given")]
    InvalidCoord(#[from] ParseFloatError),
    #[error("missing comma")]
    MissingComma,
}
// impl FromStr for waypoints {
//     type Err = ParsePointError;
//     fn from_str(env: &str) -> Result<waypoints, ParsePointError> {
//         let mut points: Vec<geo::Point> = Vec::new();
//         if let Some((lat, lon)) = env.split_once(',') {
//             let lat_float = lat.parse::<f64>()?;
//             let lon_float = lon.parse::<f64>()?;
//             points.push(geo::Point::new(lon_float, lat_float));
//         } else {
//             return Err(ParsePointError::MissingComma);
//         }
//         return Ok(waypoints(points));
//     }
// }

fn parse_point_list(wp_list: &str) -> Result<Vec<geo::Point>, ParsePointError> {
    let mut points: Vec<geo::Point> = Vec::new();
    if let Some((lat, lon)) = wp_list.split_once(',') {
        let lat_float = lat.parse::<f64>()?;
        let lon_float = lon.parse::<f64>()?;
        points.push(geo::Point::new(lon_float, lat_float));
    } else {
        return Err(ParsePointError::MissingComma);
    }
    return Ok(points);
}

#[derive(Subcommand, Debug, Clone)]
pub enum ModeRequest {
    /// plane system modes inactive
    Inactive,

    /// starts state which handles switching between capture and standby, initially starts on standby
    #[clap(subcommand)]
    Search(SearchRequest),

    /// sets the zoom control with the specific presets
    #[clap(subcommand)]
    ZoomControl(Presets),
    /// debugging mode, plane system livestreams, saving different videos for the different modes along with denoting metrics such as when each mode was switches into,
    LivestreamOnly,
}

#[derive(Error, Debug)]
pub enum ModeError {
    #[error("could not send request to the camera")]
    CameraRequestError,
    #[error("invalid waypoint entered")]
    WaypointError,
    #[error("could not send request to the gimbal")]
    GimbalRequestError,
    #[error("camera error: {0}")]
    Camera(#[from] anyhow::Error),
}

// to-do: Each preset should have a determined area to cover, behavior will be hardcodeds
#[derive(Subcommand, Debug, Clone)]
pub enum Presets {
    None,
    ExPresetName1,
    ExPresetName2,
    ExPresetName3,
    ExPresetName4,
    ExPresetName5,
}
#[derive(Subcommand, Debug, Clone)]
pub enum SearchRequest {
    //Captures for a given active interval and stays inactive for a given inactive interval
    Time {
        active: u64,   //time measured in seconds
        inactive: u64, //time measured in seconds
    },
    //Activates search when in a given range of a waypoint, deactivates when exiting range
    Distance {
        distance: u64, //distance measured in meters
        #[clap(value_parser = parse_point_list)]
        waypoint: Vec<geo::Point>, //coordinates in [lat,lon]
    },
    //Switches between active and inactive cature are handled by the user
    Manual {
        start: bool, //whether to start or end continous capture (cc)
    },
    Panning {
        //does a sweeping pan, takes given number of images during ss
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ModeResponse {
    Response,
}