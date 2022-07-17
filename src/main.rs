use crate::datastructures::config::Config;
use crate::datastructures::{FromQueryString, NotifyClientEnterView, NotifyClientLeftView};
use crate::socketlib::SocketConn;
use anyhow::anyhow;
use clap::{arg, Command};
use log::{debug, error, info, trace, warn, LevelFilter};
use std::collections::HashMap;
use std::fmt::Formatter;
use std::hint::unreachable_unchecked;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use teloxide::prelude::*;
use teloxide::types::ParseMode;
use tokio::sync::{mpsc, watch, Mutex};
use tokio::time::sleep;

mod datastructures;
mod socketlib;

async fn init_connection(
    server: String,
    port: u16,
    user: &str,
    password: &str,
    sid: i64,
) -> anyhow::Result<SocketConn> {
    let mut conn = SocketConn::connect(&server, port).await?;
    conn.login(user, password)
        .await
        .map_err(|e| anyhow!("Login failed. {:?}", e))?;

    conn.select_server(sid)
        .await
        .map_err(|e| anyhow!("Select server id failed: {:?}", e))?;

    Ok(conn)
}

enum TelegramData {
    Enter(String, i64, String, String, String),
    Left(String, i64, String, String),
    Terminate,
}

impl TelegramData {
    fn from_left(time: String, view: &NotifyClientLeftView, nickname: String) -> Self {
        Self::Left(time, view.client_id(), nickname, view.reason().to_string())
    }
    fn from_enter(time: String, view: NotifyClientEnterView) -> Self {
        Self::Enter(
            time,
            view.client_id(),
            view.client_unique_identifier().to_string(),
            view.client_nickname().to_string(),
            view.client_country().to_string(),
        )
    }
}

impl std::fmt::Display for TelegramData {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TelegramData::Enter(time, client_id, client_identifier, nickname, country) => {
                write!(
                    f,
                    "[{}] <b>{}</b>(<code>{}</code>:{})[{}] joined",
                    time,
                    nickname,
                    client_identifier,
                    client_id,
                    country_emoji::flag(country).unwrap_or_else(|| country.to_string())
                )
            }
            TelegramData::Left(time, client_id, nickname, reason) => {
                if reason.is_empty() {
                    return write!(f, "[{}] <b>{}</b>({}) left", time, nickname, client_id);
                }
                write!(
                    f,
                    "[{}] <b>{}</b>({}) left ({})",
                    time, nickname, client_id, reason
                )
            }
            TelegramData::Terminate => unsafe {
                unreachable_unchecked();
            },
        }
    }
}

async fn telegram_thread(
    token: String,
    target: i64,
    server: String,
    mut receiver: mpsc::Receiver<TelegramData>,
) -> anyhow::Result<()> {
    if token.is_empty() {
        warn!("Token is empty, skipped all send message request.");
        while let Some(cmd) = receiver.recv().await {
            if let TelegramData::Terminate = cmd {
                break;
            }
        }
        return Ok(());
    }
    let bot = Bot::new(token).set_api_url(server.parse()?);

    let bot = bot.parse_mode(ParseMode::Html);
    while let Some(cmd) = receiver.recv().await {
        if let TelegramData::Terminate = cmd {
            break;
        }
        let payload = bot.send_message(ChatId(target), cmd.to_string());
        if let Err(e) = payload.send().await {
            error!("Got error in send message {:?}", e);
        }
    }
    debug!("Send message daemon exiting...");
    Ok(())
}

async fn staff_thread(
    mut conn: SocketConn,
    recv: watch::Receiver<bool>,
    sender: mpsc::Sender<TelegramData>,
    interval: u64,
    notify_signal: Arc<Mutex<bool>>,
    ignore_list: Vec<String>,
) -> anyhow::Result<()> {
    let mut client_map: HashMap<i64, (String, bool)> = HashMap::new();
    for client in conn
        .query_clients()
        .await
        .map_err(|e| anyhow!("QueryClient failure: {:?}", e))?
    {
        if client_map.get(&client.client_id()).is_some() || client.client_type() == 1 {
            continue;
        }

        client_map.insert(
            client.client_id(),
            (client.client_nickname().to_string(), false),
        );
    }

    conn.register_events()
        .await
        .map_err(|e| anyhow!("Got error while register events: {:?}", e))?;

    debug!("Loop running!");

    loop {
        if recv
            .has_changed()
            .map_err(|e| anyhow!("Got error in check watcher {:?}", e))?
        {
            info!("Exit from staff thread!");
            conn.logout().await.ok();
            break;
        }
        let data = conn
            .read_data()
            .await
            .map_err(|e| anyhow!("Got error while read data: {:?}", e))?;

        if data.is_none() {
            let mut signal = notify_signal.lock().await;
            if *signal {
                conn.write_data("whoami\n\r")
                    .await
                    .map_err(|e| {
                        error!("Got error while write data in keep alive function: {:?}", e)
                    })
                    .ok();
                *signal = false;
            }
            continue;
        }
        let data = data.unwrap();
        let current_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        for line in data.lines() {
            trace!("{}", line);
            if line.starts_with("notifycliententerview") {
                let view = NotifyClientEnterView::from_query(line)
                    .map_err(|e| anyhow!("Got error while deserialize data: {:?}", e))?;
                let is_server_query = view.client_unique_identifier().eq("ServerQuery")
                    || ignore_list
                        .iter()
                        .any(|element| element.eq(view.client_unique_identifier()));
                client_map.insert(
                    view.client_id(),
                    (view.client_nickname().to_string(), is_server_query),
                );
                if is_server_query {
                    continue;
                }
                sender
                    .send(TelegramData::from_enter(current_time.clone(), view))
                    .await
                    .map_err(|_| error!("Got error while send data to telegram"))
                    .ok();
            }
            if line.starts_with("notifyclientleftview") {
                let view = NotifyClientLeftView::from_query(line)
                    .map_err(|e| anyhow!("Got error while deserialize data: {:?}", e))?;
                if !client_map.contains_key(&view.client_id()) {
                    warn!("Can't find client: {:?}", view.client_id());
                    continue;
                }
                let nickname = client_map.get(&view.client_id()).unwrap();
                if nickname.1 {
                    continue;
                }
                sender
                    .send(TelegramData::from_left(
                        current_time.clone(),
                        &view,
                        nickname.0.clone(),
                    ))
                    .await
                    .map_err(|_| error!("Got error while send data to telegram"))
                    .ok();
                client_map.remove(&view.client_id());
            }
        }
        sleep(Duration::from_millis(interval)).await;
    }
    sender
        .send(TelegramData::Terminate)
        .await
        .map_err(|_| error!("Got error while send terminate signal"))
        .ok();
    Ok(())
}

async fn observer(conn: SocketConn, config: Config) -> anyhow::Result<()> {
    let (exit_sender, exit_receiver) = watch::channel(false);
    let (telegram_sender, telegram_receiver) = mpsc::channel(4096);

    let keepalive_signal = Arc::new(Mutex::new(false));
    let alt_signal = keepalive_signal.clone();

    let staff_handler = tokio::spawn(staff_thread(
        conn,
        exit_receiver,
        telegram_sender,
        config.misc().interval(),
        alt_signal,
        config.server().ignore_user_name(),
    ));
    let telegram_handler = tokio::spawn(telegram_thread(
        config.telegram().api_key().to_string(),
        config.telegram().target(),
        config.telegram().api_server(),
        telegram_receiver,
    ));

    tokio::select! {
        _ = async {
            tokio::signal::ctrl_c().await.unwrap();
            info!("Recv SIGINT, send signal to thread.");
            exit_sender.send(true).unwrap();
            tokio::signal::ctrl_c().await.unwrap();
            error!("Force exit program.");
            std::process::exit(137);
        } => {
        }
        _ = async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let mut i = keepalive_signal.lock().await;
                *i = true;
            }
        } => {}
        ret = staff_handler => {
            ret??
        }
    }
    tokio::select! {
        _ = async {
            tokio::signal::ctrl_c().await.unwrap();
            error!("Force exit program.");
            std::process::exit(137);
        } => {

        }
        ret = telegram_handler => {
            ret??;
        }
    }
    Ok(())
}

async fn configure_file_bootstrap<P: AsRef<Path>>(path: P) -> anyhow::Result<()> {
    let config = Config::try_from(path.as_ref())?;
    observer(
        init_connection(
            config.raw_query().server(),
            config.raw_query().port(),
            config.raw_query().user(),
            config.raw_query().password(),
            config.server().server_id(),
        )
        .await?,
        config,
    )
    .await
}

fn main() -> anyhow::Result<()> {
    let matches = Command::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .args(&[arg!([CONFIG_FILE] "Override default configure file location")])
        .get_matches();

    env_logger::Builder::from_default_env()
        .filter_module("rustls", LevelFilter::Warn)
        .filter_module("reqwest", LevelFilter::Warn)
        .init();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(configure_file_bootstrap(
            matches.value_of("CONFIG_FILE").unwrap_or("config.toml"),
        ))?;
    Ok(())
}
