use crate::camera::Camera;
use smol::lock::Mutex;
use std::sync::Arc;
use tide::{self, Request};

#[derive(Clone)]
struct ServerState {
    camera: Arc<Mutex<Option<Camera>>>,
}

pub async fn serve() -> Result<(), std::io::Error> {
    info!("initializing server");

    let state = ServerState {
        camera: Arc::new(Mutex::new(None)),
    };

    let mut app = tide::with_state(state);

    app.at("/").get(|_| async {
        let response = tide::Response::builder(200)
            .body(tide::Body::empty())
            .content_type("text/plain")
            .build();
        Ok(response)
    });

    app.at("/connect")
        .get(|req: Request<ServerState>| async move {
            let camera = req.state().camera.clone();

            if camera.lock().await.is_some() {
                Ok("already connected")
            } else {
                smol::spawn(connect_camera(camera)).detach();
                Ok("working")
            }
        });

    app.at("/disconnect")
        .get(|req: Request<ServerState>| async move {
            let camera = req.state().camera.clone();

            if camera.lock().await.is_none() {
                Ok("already disconnected")
            } else {
                smol::spawn(disconnect_camera(camera)).detach();
                Ok("working")
            }
        });

    let address = "127.0.0.1:8080";
    info!("initialized server");
    info!("listening at {:#?}", address);

    app.listen(address).await?;
    Ok(())
}

async fn connect_camera(camera: Arc<Mutex<Option<Camera>>>) {
    info!("connecting to camera");
    
}

async fn disconnect_camera(camera: Arc<Mutex<Option<Camera>>>) {
    info!("disconnecting from to camera");
}
