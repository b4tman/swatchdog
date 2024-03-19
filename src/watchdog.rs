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
    verbose: bool,
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

    if params.verbose {
        eprint!("{} {} -> ", params.method, url);
    }

    let result = params
        .client
        .request(params.method.clone(), url)
        .send()
        .and_then(|res| res.error_for_status());

    if let Err(err) = result {
        eprintln!("Error: {}", err)
    } else if params.verbose {
        eprintln!("Success");
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
    verbose: bool,
    host: String,
    shutdown_rx: mpsc::Receiver<Nothing>,
}

impl Watchdog {
    pub fn new(
        url: reqwest::Url,
        method: Method,
        interval: Duration,
        verbose: bool,
        shutdown_rx: mpsc::Receiver<Nothing>,
    ) -> Watchdog {
        let url = Url::parse(url.as_str()).expect("parse url");
        let host: String = url.host().expect("no host in url").to_string();
        Watchdog {
            url,
            method,
            interval,
            verbose,
            host,
            shutdown_rx,
        }
    }

    pub fn run(self) {
        let params = SenderParams {
            client: Client::new(),
            url: self.url.clone(),
            method: self.method.clone(),
            verbose: self.verbose,
            interval: self.interval,
        };

        let (tx, rx) = mpsc::sync_channel::<Message>(1);
        [
            thread::spawn(move || {
                info_getter_thread(self.host, self.interval, tx, self.shutdown_rx)
            }),
            thread::spawn(move || heartbeat_sender_thread(params, rx)),
        ]
        .into_iter()
        .for_each(|handle| handle.join().expect("thread panic"));
    }
}
