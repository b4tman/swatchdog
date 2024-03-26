use anyhow::{anyhow, Context, Result};
use humantime::format_duration;
use pinger::ping;
use reqwest::blocking::Client;
use reqwest::Method;
use std::cmp::min;
use std::net::IpAddr;
use std::thread;
use std::{
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant},
};
use sysinfo::System;
use url::Url;

use crate::args;

enum Message {
    HostInfo(String, String),
}

struct SenderParams {
    client: Client,
    url: Url,
    method: Method,
    interval: Duration,
}

fn get_uptime() -> String {
    let dur = Duration::from_secs(System::uptime());
    format!("up {}", format_duration(dur))
}

fn ping_host(host: &str) -> Result<Duration> {
    let stream = ping(host.into(), None)?;
    if let Ok(pinger::PingResult::Pong(duration, _)) = stream.recv() {
        return Ok(duration);
    }
    Err(anyhow!("ping error"))
}

fn info_getter_thread(
    host: String,
    interval: Duration,
    tx: mpsc::SyncSender<Message>,
    shutdown_rx: mpsc::Receiver<()>,
) {
    let mut measure_time = Duration::new(0, 0);
    loop {
        match shutdown_rx.recv_timeout(interval - measure_time) {
            Ok(_) | Err(RecvTimeoutError::Disconnected) => {
                break;
            }
            Err(RecvTimeoutError::Timeout) => {
                let start = Instant::now();

                let mut ping = String::new();
                let ping_result = ping_host(&host);
                if let Ok(duration) = ping_result {
                    ping = format!("{:?}", duration);
                }
                let uptime = get_uptime();

                let end = Instant::now();
                measure_time = min(end - start, interval - Duration::from_millis(1));

                let res = tx.send(Message::HostInfo(uptime, ping));
                if res.is_err() {
                    break;
                }
            }
        }
    }
}

fn send_heartbeat(params: &SenderParams, uptime: &str, ping: &str) {
    let mut url = params.url.clone();
    url.query_pairs_mut()
        .clear()
        .append_pair("status", "up")
        .append_pair("msg", uptime)
        .append_pair("ping", ping);

    log::info!("{} {}", params.method, url);

    let result = params
        .client
        .request(params.method.clone(), url)
        .send()
        .and_then(|res| res.error_for_status());

    if let Err(err) = result {
        log::error!("Error: {}", err)
    } else {
        log::info!("Success");
    }
}

fn heartbeat_sender_thread(params: SenderParams, rx: mpsc::Receiver<Message>) {
    let mut last_uptime: String = String::new();
    let mut last_ping: String = String::new();
    loop {
        match rx.recv_timeout(params.interval + Duration::from_millis(100)) {
            Err(RecvTimeoutError::Disconnected) => break,
            Ok(Message::HostInfo(uptime, ping)) => {
                last_uptime = uptime;
                last_ping = ping;
                send_heartbeat(&params, &last_uptime, &last_ping);
            }
            Err(RecvTimeoutError::Timeout) => send_heartbeat(&params, &last_uptime, &last_ping),
        }
    }
}

pub fn create_shutdown_chanel() -> (mpsc::SyncSender<()>, mpsc::Receiver<()>) {
    mpsc::sync_channel::<()>(1)
}

pub struct Watchdog {
    url: reqwest::Url,
    method: Method,
    interval: Duration,
    host: String,
    ignore_cert_errors: bool,
    local_address: Option<IpAddr>,
    shutdown_tx: Option<mpsc::SyncSender<()>>,
    shutdown_rx: mpsc::Receiver<()>,
}

impl TryFrom<args::Args> for Watchdog {
    type Error = anyhow::Error;

    fn try_from(args: args::Args) -> std::prelude::v1::Result<Self, Self::Error> {
        let url = Url::parse(args.url.as_str()).context("parse url")?;
        let host: String = url.host().context("no host in url")?.to_string();

        let (shutdown_tx, shutdown_rx) = create_shutdown_chanel();
        let shutdown_tx = Some(shutdown_tx);

        if !url.scheme().contains("http") {
            return Err(anyhow!("URL scheme is not allowed: {}", url.scheme()));
        }

        Ok(Watchdog {
            url,
            method: args.method,
            interval: args.interval,
            host,
            ignore_cert_errors: args.insecure,
            local_address: args.local_address,
            shutdown_tx,
            shutdown_rx,
        })
    }
}

impl Watchdog {
    pub fn take_shutdown_tx(&mut self) -> Option<mpsc::SyncSender<()>> {
        self.shutdown_tx.take()
    }
    pub fn run(self) -> Result<()> {
        let params = SenderParams {
            client: reqwest::blocking::Client::builder()
                .danger_accept_invalid_certs(self.ignore_cert_errors)
                .local_address(self.local_address)
                .build()?,
            url: self.url,
            method: self.method,
            interval: self.interval,
        };

        let (tx, rx) = mpsc::sync_channel::<Message>(1);
        let handles = [
            thread::spawn(move || {
                info_getter_thread(self.host, self.interval, tx, self.shutdown_rx)
            }),
            thread::spawn(move || heartbeat_sender_thread(params, rx)),
        ];
        for handle in handles {
            handle
                .join()
                .map_err(|e| anyhow!("thread panic: {:?}", e))?
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn send_heartbeat_test() {
        use httptest::{matchers::*, responders::*, Expectation, Server};
        let server = Server::run();
        server.expect(
            Expectation::matching(all_of![
                request::method_path("GET", "/foo"),
                request::query(url_decoded(all_of![
                    contains(("status", "up")),
                    contains(("msg", "test_uptime")),
                    contains(("ping", "test_ping")),
                ])),
            ])
            .respond_with(status_code(200)),
        );

        let url: Url = server.url("/foo").to_string().parse().unwrap();
        let client = Client::new();
        let params = SenderParams {
            client,
            url,
            method: Method::GET,
            interval: Duration::from_millis(0),
        };
        send_heartbeat(&params, "test_uptime", "test_ping");

        // on Drop the server will assert all expectations have been met and will panic if not.
    }

    #[test]
    fn shutdown_test() {
        let (tx, rx) = create_shutdown_chanel();
        let mut wd = Watchdog {
            url: "http://localhost".parse().unwrap(),
            method: Method::GET,
            interval: Duration::from_millis(100),
            host: "localhost".parse().unwrap(),
            ignore_cert_errors: true,
            local_address: None,
            shutdown_tx: Some(tx),
            shutdown_rx: rx,
        };

        let mut shutdown = wd.take_shutdown_tx();

        let t = thread::spawn(move || wd.run());
        shutdown.take();
        thread::sleep(Duration::from_millis(50));
        assert!(t.is_finished());
    }

    #[test]
    fn get_uptime_test() {
        let uptime1 = get_uptime();
        assert!(uptime1.starts_with("up "));

        thread::sleep(Duration::from_secs(1));
        let uptime2 = get_uptime();
        assert_ne!(uptime1, uptime2);
    }

    #[test]
    fn ping_localhost() {
        ping_host("localhost").unwrap();
    }
}
