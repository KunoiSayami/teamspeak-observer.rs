pub trait FromQueryString: for<'de> Deserialize<'de> {
    fn from_query(data: &str) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        serde_teamspeak_querystring::from_str(data)
            .map_err(|e| anyhow::anyhow!("Got parser error: {:?}", e))
    }
}

pub mod client {
    use super::FromQueryString;
    use serde_derive::Deserialize;

    #[allow(dead_code)]
    #[derive(Clone, Debug, Default, Deserialize)]
    pub struct Client {
        clid: i64,
        cid: i64,
        client_database_id: i64,
        client_type: i64,
        //client_unique_identifier: String,
        client_nickname: String,
    }

    #[allow(dead_code)]
    impl Client {
        pub fn client_id(&self) -> i64 {
            self.clid
        }
        pub fn channel_id(&self) -> i64 {
            self.cid
        }
        pub fn client_database_id(&self) -> i64 {
            self.client_database_id
        }
        pub fn client_type(&self) -> i64 {
            self.client_type
        }
        pub fn client_unique_identifier(&self) -> String {
            format!("{}", self.client_database_id)
        }
        pub fn client_nickname(&self) -> &str {
            &self.client_nickname
        }
    }

    impl FromQueryString for Client {}

    #[cfg(test)]
    mod test {
        use crate::datastructures::client::Client;
        use crate::datastructures::FromQueryString;

        const TEST_STRING: &str = "clid=8 cid=1 client_database_id=1 client_nickname=serveradmin client_type=1 client_unique_identifier=serveradmin";

        #[test]
        fn test() {
            let result = Client::from_query(TEST_STRING).unwrap();
            assert_eq!(result.client_id(), 8);
            assert_eq!(result.channel_id(), 1);
            assert_eq!(result.client_database_id(), 1);
            assert_eq!(result.client_nickname(), "serveradmin".to_string());
            assert_eq!(result.client_type(), 1);
            assert_eq!(result.client_unique_identifier(), "serveradmin".to_string());
        }
    }
}

pub mod notifies {
    use crate::datastructures::FromQueryString;
    use serde_derive::Deserialize;

    #[derive(Clone, Debug, Deserialize)]
    pub struct NotifyClientEnterView {
        #[serde(rename = "clid")]
        client_id: i64,
        client_nickname: String,
        client_unique_identifier: String,
        client_country: String,
    }

    impl NotifyClientEnterView {
        pub fn client_id(&self) -> i64 {
            self.client_id
        }
        pub fn client_nickname(&self) -> &str {
            &self.client_nickname
        }
        pub fn client_country(&self) -> &str {
            &self.client_country
        }
        pub fn client_unique_identifier(&self) -> &str {
            &self.client_unique_identifier
        }
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct NotifyClientLeftView {
        #[serde(rename = "clid")]
        client_id: i64,
        #[serde(rename = "reasonmsg", default)]
        reason: String,
    }

    impl NotifyClientLeftView {
        pub fn client_id(&self) -> i64 {
            self.client_id
        }
        pub fn reason(&self) -> &str {
            &self.reason
        }
    }

    impl FromQueryString for NotifyClientEnterView {}
    impl FromQueryString for NotifyClientLeftView {}
}

pub mod query_status {
    use crate::datastructures::{QueryError, QueryResult};
    use anyhow::anyhow;
    use serde_derive::Deserialize;

    #[derive(Clone, Debug, Deserialize)]
    pub struct WebQueryStatus {
        code: i32,
        message: String,
    }

    impl WebQueryStatus {
        pub fn into_status(self) -> QueryStatus {
            QueryStatus {
                id: self.code,
                msg: self.message,
            }
        }
    }

    impl From<WebQueryStatus> for QueryStatus {
        fn from(status: WebQueryStatus) -> Self {
            status.into_status()
        }
    }

    #[allow(dead_code)]
    #[derive(Clone, Debug, Deserialize)]
    pub struct QueryStatus {
        id: i32,
        msg: String,
    }

    impl Default for QueryStatus {
        fn default() -> Self {
            Self {
                id: 0,
                msg: "ok".to_string(),
            }
        }
    }

    impl QueryStatus {
        pub fn id(&self) -> i32 {
            self.id
        }
        pub fn msg(&self) -> &String {
            &self.msg
        }

        pub fn into_err(self) -> QueryError {
            QueryError::from(self)
        }

        pub fn into_result<T>(self, ret: T) -> QueryResult<T> {
            if self.id == 0 {
                return Ok(ret);
            }
            Err(self.into_err())
        }
    }

    impl TryFrom<&str> for QueryStatus {
        type Error = anyhow::Error;

        fn try_from(value: &str) -> Result<Self, Self::Error> {
            let (_, line) = value
                .split_once("error ")
                .ok_or_else(|| anyhow!("Split error: {}", value))?;
            serde_teamspeak_querystring::from_str(line)
                .map_err(|e| anyhow!("Got error while parse string: {:?} {:?}", line, e))
        }
    }
}

pub mod config {
    use anyhow::anyhow;
    use serde_derive::Deserialize;
    use std::fs::read_to_string;
    use std::path::Path;

    #[derive(Clone, Debug, Deserialize)]
    pub struct RawQuery {
        server: Option<String>,
        port: Option<u16>,
        user: String,
        password: String,
    }

    impl RawQuery {
        pub fn server(&self) -> String {
            if let Some(server) = &self.server {
                server.clone()
            } else {
                String::from("127.0.0.1")
            }
        }
        pub fn port(&self) -> u16 {
            self.port.unwrap_or(10011)
        }
        pub fn user(&self) -> &str {
            &self.user
        }
        pub fn password(&self) -> &str {
            &self.password
        }
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct Server {
        server_id: Option<i64>,
        ignore_user: Option<Vec<String>>,
    }

    impl Server {
        pub fn server_id(&self) -> i64 {
            self.server_id.unwrap_or(1)
        }
        pub fn ignore_user_name(&self) -> Vec<String> {
            self.ignore_user.clone().unwrap_or_default()
        }
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct Misc {
        interval: Option<u64>,
    }

    impl Misc {
        pub fn interval(&self) -> u64 {
            self.interval.unwrap_or(20)
        }
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct Telegram {
        api_key: String,
        api_server: Option<String>,
        target: i64,
    }

    impl Telegram {
        pub fn api_key(&self) -> &str {
            &self.api_key
        }
        pub fn api_server(&self) -> String {
            if let Some(server) = &self.api_server {
                return server.clone();
            }
            String::from("https://api.telegram.org/")
        }
        pub fn target(&self) -> i64 {
            self.target
        }
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct Config {
        server: Server,
        misc: Misc,
        telegram: Telegram,
        raw_query: RawQuery,
    }

    impl Config {
        pub fn server(&self) -> &Server {
            &self.server
        }
        pub fn misc(&self) -> &Misc {
            &self.misc
        }
        pub fn raw_query(&self) -> &RawQuery {
            &self.raw_query
        }
        pub fn telegram(&self) -> &Telegram {
            &self.telegram
        }
    }

    impl TryFrom<&Path> for Config {
        type Error = anyhow::Error;

        fn try_from(path: &Path) -> Result<Self, Self::Error> {
            let content = read_to_string(path).map_err(|e| anyhow!("Read error: {:?}", e))?;

            toml::from_str(&content).map_err(|e| anyhow!("Deserialize toml error: {:?}", e))
        }
    }
}

mod status_result {
    use crate::datastructures::QueryStatus;
    use anyhow::Error;
    use std::fmt::{Display, Formatter};

    pub type QueryResult<T> = Result<T, QueryError>;

    #[derive(Clone, Default, Debug)]
    pub struct QueryError {
        code: i32,
        message: String,
    }

    impl QueryError {
        #[allow(unused)]
        pub fn static_empty_response() -> Self {
            Self {
                code: -1,
                message: "Expect result but none found.".to_string(),
            }
        }
        #[allow(unused)]
        pub fn code(&self) -> i32 {
            self.code
        }
    }

    impl Display for QueryError {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}({})", self.message, self.code)
        }
    }

    impl std::error::Error for QueryError {}

    impl From<QueryStatus> for QueryError {
        fn from(status: QueryStatus) -> Self {
            Self {
                code: status.id(),
                message: status.msg().clone(),
            }
        }
    }

    impl From<Error> for QueryError {
        fn from(s: Error) -> Self {
            Self {
                code: -2,
                message: s.to_string(),
            }
        }
    }
}

pub use client::Client;
pub use notifies::{NotifyClientEnterView, NotifyClientLeftView};
pub use query_status::{QueryStatus, WebQueryStatus};
use serde::Deserialize;
pub use status_result::{QueryError, QueryResult};
