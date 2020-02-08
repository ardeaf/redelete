use serde::{Deserialize, Serialize};
use std::cmp::{Eq, PartialEq};
use std::collections::HashMap;
use std::net::TcpListener;
use tiny_http::Server;
use url::Url;

#[cfg(test)]
use reqwest::Client;
#[cfg(test)]
use tokio::runtime::Runtime;

#[derive(Deserialize, Serialize, PartialEq, Eq, Debug)]
pub struct OAuthRedirect {
    pub code: String,
    pub state: String,
}

fn port_is_available(port: u16) -> bool {
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(_) => true,
        Err(_) => false,
    }
}
fn get_available_port(start: u16, end: u16) -> Option<u16> {
    (start..end).find(|port| port_is_available(*port))
}

fn parse_oauth_redirect(query_params: HashMap<String, String>) -> Option<OAuthRedirect> {
    // sample hashmap: {"error": "errors", "code": "nYMT0zn-3hh2-efn2j18BN-an8Z", "/?state": "UnK1pO8pbyEGRw0iYwUrt"}
    if query_params.contains_key("error") {
        match &*query_params["error"] {
            "access_denied" => println!("User declined authorization of redelete."),
            "unsupported_response_type" => println!("response_type parameter was incorrect"),
            "invalid_scope" => println!("Scope was invalid"),
            "invalid_request" => println!("Authorization request was invalid"),
            s => println!("Unknown error: {}", s),
        }
        None
    } else {
        Some(OAuthRedirect {
            code: String::from(&query_params["code"]),
            state: String::from(&query_params["state"]),
        })
    }
}

pub fn wait_for_oauth_redirect(
    start: u16,
    end: u16,
) -> Result<OAuthRedirect, Box<dyn std::error::Error>> {
    let port = get_available_port(start, end).expect(&format!(
        "Please open up port 8000 and rerun the authorization." // "Unable to find an open port in range {} to {}, please open up a port.",
                                                                // start, end
    ));
    let server = Server::http(format!("0.0.0.0:{}", port))
        .expect("Could not start tiny_http server for oauth2 authentication.");
    let request = server
        .recv()
        .expect("Something went wrong unwrapping the request received from the oauth2 redirect.");
    let params: HashMap<String, String> = Url::query_pairs(
        &Url::parse(&format!("http://localhost:{}{}", port, request.url())).expect(&format!(
            "Unable to parse redirect querystring: {}",
            request.url()
        )),
    )
    .into_owned()
    .collect();
    let oauth_redirect = parse_oauth_redirect(params);
    Ok(oauth_redirect.expect("Error during oauth2 redirect."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_available_port() {
        // make sure you've got at least one open port for this!
        let (start, end) = (0, 65535);
        let port = get_available_port(start, end).unwrap();
        assert_eq!(port >= start && port <= end, true);
    }

    #[test]
    fn test_run() {
        let (start, end) = (8000, 9000);
        let test_header_value = "TEST HEADER";
        let port = get_available_port(start, end).unwrap();
        // Spawning the server in a separate thread
        let server = std::thread::spawn(move || wait_for_oauth_redirect(start, end).unwrap());
        let test_oauth_redirect = OAuthRedirect {
            code: "test_code".into(),
            state: "test_state".into(),
        };
        // Send the redirect to our local server
        Runtime::new().unwrap().block_on(async {
            let client = Client::new();
            let url = format!(
                "http://localhost:{}/authorize?code=test_code&state=test_state",
                port
            );
            let body: String = serde_json::to_string(&test_oauth_redirect).unwrap();
            let _res = client
                .get(&url)
                .header("test", test_header_value)
                .body(body)
                .send()
                .await
                .unwrap();
        });
        let oauth_redirect_data = server.join().unwrap();
        assert_eq!(test_oauth_redirect, oauth_redirect_data)
    }

    #[test]
    #[should_panic]
    fn test_closed_ports() {
        let test_port = get_available_port(8500, 9000).unwrap();
        let _tcp = TcpListener::bind(("127.0.0.1", test_port)).unwrap();
        wait_for_oauth_redirect(test_port, test_port + 1).unwrap();
    }
}
