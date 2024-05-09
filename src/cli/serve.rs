use std::{
    fs::File,
    net::{Ipv4Addr, SocketAddr},
};
use thought::{Result, Workspace};
use tiny_http::{Header, Response, Server, StatusCode};
pub fn command(port: Option<u16>) -> Result<()> {
    let workspace = Workspace::current()?;
    let server = Server::http(SocketAddr::new(
        Ipv4Addr::new(127, 0, 0, 1).into(),
        port.unwrap_or(8080),
    ))
    .unwrap();

    log::info!("Serve at 127.0.0.1:8080");

    for request in server.incoming_requests() {
        let mut path = workspace.generate_path().join(&request.url()[1..]);
        if path.is_dir() {
            path.push("index.html");
        }

        let file = File::open(&path);
        match file {
            Ok(file) => {
                let mut response = Response::from_file(file);

                response.add_header(
                    Header::from_bytes(
                        "Content-Type",
                        mime_guess::from_path(path)
                            .first()
                            .unwrap_or(mime::TEXT_PLAIN_UTF_8)
                            .to_string(),
                    )
                    .unwrap(),
                );
                request.respond(response)?;
            }
            Err(error) => {
                log::error!("Cannot open file: {error}");
                let response = Response::empty(StatusCode(404));
                request.respond(response)?;
            }
        }
    }
    Ok(())
}
