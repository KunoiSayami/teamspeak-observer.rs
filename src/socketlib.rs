use crate::datastructures::{Client, QueryResult};
use crate::datastructures::{FromQueryString, QueryStatus};
use anyhow::anyhow;
use log::{error, warn};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const BUFFER_SIZE: usize = 512;

pub struct SocketConn {
    conn: TcpStream,
}

impl SocketConn {
    fn decode_status(content: String) -> QueryResult<String> {
        debug_assert!(
            !content.contains("Welcome to the TeamSpeak 3") && content.contains("error id="),
            "Content => {:?}",
            content
        );

        for line in content.lines() {
            if line.trim().starts_with("error ") {
                let status = QueryStatus::try_from(line)?;

                return status.into_result(content);
            }
        }
        panic!("Should return status in reply => {}", content)
    }

    fn decode_status_with_result<T: FromQueryString + Sized>(
        data: String,
    ) -> QueryResult<Option<Vec<T>>> {
        let content = Self::decode_status(data)?;

        for line in content.lines() {
            if !line.starts_with("error ") {
                let mut v = Vec::new();
                for element in line.split('|') {
                    v.push(T::from_query(element)?);
                }
                return Ok(Some(v));
            }
        }
        Ok(None)
    }

    pub async fn read_data(&mut self) -> anyhow::Result<Option<String>> {
        let mut buffer = [0u8; BUFFER_SIZE];
        let mut ret = String::new();
        loop {
            let size = if let Ok(data) =
                tokio::time::timeout(Duration::from_secs(2), self.conn.read(&mut buffer)).await
            {
                match data {
                    Ok(size) => size,
                    Err(e) => return Err(anyhow!("Got error while read data: {:?}", e)),
                }
            } else {
                return Ok(None);
            };

            ret.push_str(&String::from_utf8_lossy(&buffer[..size]));
            if size < BUFFER_SIZE || (ret.contains("error id=") && ret.ends_with("\n\r")) {
                break;
            }
        }
        Ok(if ret.is_empty() { None } else { Some(ret) })
    }

    pub async fn write_data(&mut self, payload: &str) -> anyhow::Result<()> {
        debug_assert!(payload.ends_with("\n\r"));
        self.conn
            .write(payload.as_bytes())
            .await
            .map(|size| {
                if size != payload.as_bytes().len() {
                    error!(
                        "Error payload size mismatch! expect {} but {} found. payload: {:?}",
                        payload.as_bytes().len(),
                        size,
                        payload
                    )
                }
            })
            .map_err(|e| anyhow!("Got error while send data: {:?}", e))?;
        Ok(())
    }

    async fn write_and_read(&mut self, payload: &str) -> anyhow::Result<String> {
        self.write_data(payload).await?;
        self.read_data()
            .await?
            .ok_or_else(|| anyhow!("Return data is None"))
    }

    async fn basic_operation(&mut self, payload: &str) -> QueryResult<()> {
        let data = self.write_and_read(payload).await?;
        Self::decode_status(data).map(|_| ())
    }

    async fn query_operation_non_error<T: FromQueryString + Sized>(
        &mut self,
        payload: &str,
    ) -> QueryResult<Vec<T>> {
        let data = self.write_and_read(payload).await?;
        let ret = Self::decode_status_with_result(data)?;
        Ok(ret
            .ok_or_else(|| panic!("Can't find result line, payload => {}", payload))
            .unwrap())
    }

    #[allow(unused)]
    async fn query_operation<T: FromQueryString + Sized>(
        &mut self,
        payload: &str,
    ) -> QueryResult<Option<Vec<T>>> {
        let data = self.write_and_read(payload).await?;
        Self::decode_status_with_result(data)
        //let status = status.ok_or_else(|| anyhow!("Can't find status line."))?;
    }

    pub async fn connect(server: &str, port: u16) -> anyhow::Result<Self> {
        let conn = TcpStream::connect(format!("{}:{}", server, port))
            .await
            .map_err(|e| anyhow!("Got error while connect to {}:{} {:?}", server, port, e))?;

        //let bufreader = BufReader::new(conn);
        //conn.set_nonblocking(true).unwrap();
        let mut self_ = Self { conn };

        let content = self_
            .read_data()
            .await
            .map_err(|e| anyhow!("Got error in connect while read content: {:?}", e))?;

        if content.is_none() {
            warn!("Read none data.");
        }

        Ok(self_)
    }

    pub async fn login(&mut self, user: &str, password: &str) -> QueryResult<()> {
        let payload = format!("login {} {}\n\r", user, password);
        self.basic_operation(payload.as_str()).await
    }

    pub async fn select_server(&mut self, server_id: i64) -> QueryResult<()> {
        let payload = format!("use {}\n\r", server_id);
        self.basic_operation(payload.as_str()).await
    }

    pub async fn query_clients(&mut self) -> QueryResult<Vec<Client>> {
        self.query_operation_non_error("clientlist\n\r").await
    }

    pub async fn logout(&mut self) -> anyhow::Result<()> {
        self.write_data("quit\n\r").await
    }

    pub async fn register_events(&mut self) -> QueryResult<()> {
        self.basic_operation("servernotifyregister event=server\n\r")
            .await
    }
}
