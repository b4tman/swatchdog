use anyhow::{anyhow, Context, Result};
use humantime::format_duration;
use pinger::ping;
use reqwest::blocking::Client;
use reqwest::Method;
use std::cmp::min;
use std::marker::PhantomData;
use std::thread;
use std::{
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant},
};
use sysinfo::System;
use url::Url;

enum Message {
    HostInfo(String, String),
}

struct SenderParams {
    client: Client,
    url: Url,
    method: Method,
    interval: Duration,
}

pub type Nothing = PhantomData<Option<bool>>;

fn get_uptime() -> String {
    let dur = Duration::from_secs(System::uptime());
    format!("up {}", format_duration(dur))
}

fn ping_host(host: &str) -> Result<Duration, Nothing> {
    let stream = ping(host.into(), None);
    if stream.is_err() {
        return Err(PhantomData);
    }
    if let Ok(pinger::PingResult::Pong(duration, _)) = stream.unwrap().recv() {
        return Ok(duration);
    }
    Err(PhantomData)
}

fn info_getter_thread(
    host: String,
    interval: Duration,
    tx: mpsc::SyncSender<Message>,
    shutdown_rx: mpsc::Receiver<Nothing>,
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

pub struct Watchdog {
    url: reqwest::Url,
    method: Method,
    interval: Duration,
    host: String,
    shutdown_rx: mpsc::Receiver<Nothing>,
    ignore_cert_errors: bool,
}

impl Watchdog {
    pub fn new(
        url: reqwest::Url,
        method: Method,
        interval: Duration,
        shutdown_rx: mpsc::Receiver<Nothing>,
        ignore_cert_errors: bool,
    ) -> Result<Watchdog> {
        let url = Url::parse(url.as_str()).context("parse url")?;
        let host: String = url.host().context("no host in url")?.to_string();

        if !url.scheme().contains("http") {
            return Err(anyhow!("URL scheme is not allowed: {}", url.scheme()));
        }

        Ok(Watchdog {
            url,
            method,
            interval,
            host,
            shutdown_rx,
            ignore_cert_errors,
        })
    }

    pub fn run(self) -> Result<()> {
        let params = SenderParams {
            client: reqwest::blocking::Client::builder()
                .danger_accept_invalid_certs(self.ignore_cert_errors)
                .build()?,
            url: self.url.clone(),
            method: self.method.clone(),
            interval: self.interval,
        };

        let (tx, rx) = mpsc::sync_channel::<Message>(1);
        for handle in [
            thread::spawn(move || {
                info_getter_thread(self.host, self.interval, tx, self.shutdown_rx)
            }),
            thread::spawn(move || heartbeat_sender_thread(params, rx)),
        ] {
            handle
                .join()
                .map_err(|e| anyhow!("thread panic: {:?}", e))?
        }

        Ok(())
    }
}
