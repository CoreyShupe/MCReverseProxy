mod proxy;
mod srv;

use std::path::PathBuf;

use log::LevelFilter;
use srv::IntoPriorityResolver;
use tokio::net::TcpListener;
use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::{TokioAsyncResolver, TokioConnectionProvider, TokioHandle};

#[derive(Debug, Clone, serde_derive::Serialize, serde_derive::Deserialize)]
pub struct LoggerOptions {
    log_level: LevelFilter,
    #[serde(default)]
    log_file: Option<PathBuf>,
}

impl Default for LoggerOptions {
    fn default() -> Self {
        LoggerOptions {
            log_level: LevelFilter::Info,
            log_file: Some(PathBuf::from("./output.log")),
        }
    }
}

pub fn attach_system_logger(options: LoggerOptions) -> anyhow::Result<()> {
    let mut dispatcher = fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{{{}}} [{}/{}] {}",
                chrono::Local::now().format("%d/%m/%y %H:%M:%S"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(options.log_level)
        .chain(std::io::stdout());

    if let Some(path) = options.log_file.as_ref() {
        if path.exists() {
            std::fs::remove_file(path)?;
        }

        dispatcher = dispatcher.chain(fern::log_file(path)?);
    }

    dispatcher.apply()?;

    Ok(())
}

fn f() -> bool {
    false
}

#[derive(serde_derive::Serialize, serde_derive::Deserialize)]
struct Config {
    #[serde(default)]
    logger: LoggerOptions,
    target: String,
    #[serde(default = "f")]
    srv: bool,
    bind: String,
}

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    let config: Config = serde_yaml::from_reader(std::fs::File::open("config.yaml")?)?;
    attach_system_logger(config.logger)?;

    let resolver = TokioAsyncResolver::new_with_conn(
        ResolverConfig::new(),
        ResolverOpts::default(),
        TokioConnectionProvider::new(TokioHandle),
    )?;
    let path = format!("_minecraft._tcp.{}.", config.target);
    let listener = TcpListener::bind(config.bind).await?;

    'connection_loop: while let Ok(inbound) = listener.accept().await {
        let target_clone = config.target.clone();

        if config.srv {
            let lookup = resolver.srv_lookup(path.clone()).await?;
            let mut resolver = lookup.iter().priority_resolver();
            while let Some(record) = resolver.next() {
                if let Ok(outbound) =
                    proxy::connect_basic(format!("{}:{}", record.target, record.port)).await
                {
                    if let Err(err) =
                        proxy::proxy_connection(inbound.0, outbound, target_clone).await
                    {
                        log::error!("Error proxying connection: {}", err);
                    }
                    continue 'connection_loop;
                };
            }
        } else {
            let connect = proxy::connect_basic(format!("{}:25565", &target_clone)).await?;
            if let Err(err) = proxy::proxy_connection(inbound.0, connect, target_clone).await {
                log::error!("Error proxying connection: {}", err);
            }
        }
    }

    Ok(())
}
