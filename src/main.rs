use clap::Parser;
use humantime::format_duration;
use parse_duration::parse as parse_duration;
use pinger::ping;
use reqwest::blocking::Client;
use reqwest::Method;
use std::marker::PhantomData;
use std::thread;
use std::{
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant},
};
use sysinfo::System;
use url::Url;

#[derive(Parser, Debug, Clone)]
#[command(author, version)]
struct Args {
    #[arg(short, long)]
    url: reqwest::Url,
    #[arg(long, default_value = "GET")]
    method: Method,
    #[arg(long, default_value = "60s", value_parser = parse_duration)]
    interval: Duration,
    #[arg(long, default_value = "false")]
    verbose: bool,
}

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

type Nothing = PhantomData<Option<bool>>;

fn get_uptime() -> String {
    let dur = Duration::from_secs(System::uptime());
    format!("up {}", format_duration(dur))
}

fn info_getter_thread(
    host: String,
    interval: Duration,
    tx: mpsc::SyncSender<Message>,
    shutdown_rx: mpsc::Receiver<Nothing>,
) {
    let stream = ping(host, None).expect("Error pinging");
    let mut measure_time = Duration::new(0, 0);
    loop {
        match shutdown_rx.recv_timeout(interval - measure_time) {
            Ok(_) | Err(RecvTimeoutError::Disconnected) => {
                break;
            }
            Err(RecvTimeoutError::Timeout) => {
                let start = Instant::now();
                if let Ok(pinger::PingResult::Pong(duration, _)) = stream.recv() {
                    let end = Instant::now();
                    measure_time = end - start;

                    let uptime = get_uptime();
                    let ping = format!("{:?}", duration);
                    let res = tx.send(Message::HostInfo(uptime, ping));
                    if res.is_err() {
                        break;
                    }
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

fn main() {
    let args = Args::parse();
    let interval = args.interval;
    let url = args.url;
    let params = SenderParams {
        client: Client::new(),
        url: url.clone(),
        method: args.method,
        verbose: args.verbose,
        interval: args.interval,
    };
    let url = Url::parse(url.as_str()).expect("parse url");
    let host: String = url.host().expect("no host in url").to_string();

    println!("swatchdog v{} started!", env!("CARGO_PKG_VERSION"));

    let (shutdown_tx, shutdown_rx) = mpsc::sync_channel::<Nothing>(1);
    let mut shutdown = Some(shutdown_tx);

    let res = ctrlc::set_handler(move || {
        println!("recieved Ctrl-C");
        shutdown.take(); // drop shutdown_tx
    });

    if res.is_ok() {
        println!("Press Ctrl-C to stop");
    }

    let (tx, rx) = mpsc::sync_channel::<Message>(1);
    [
        thread::spawn(move || info_getter_thread(host, interval, tx, shutdown_rx)),
        thread::spawn(move || heartbeat_sender_thread(params, rx)),
    ]
    .into_iter()
    .for_each(|handle| handle.join().expect("thread panic"));
    println!("bye!");
}
