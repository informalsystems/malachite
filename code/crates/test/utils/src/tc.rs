use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::net::IpAddr;
use std::process::ExitStatus;

use surge_ping::SurgeError;
use thiserror::Error;
use tokio::time::Duration;

#[derive(Error, Debug)]
pub enum TcError {
    #[error("Failed to execute tc command: {0}")]
    Command(#[from] std::io::Error),

    #[error("tc command failed with status {0}: {1}")]
    Tc(ExitStatus, String),

    #[error("Ping failed: {0}")]
    Ping(#[from] SurgeError),
}

pub struct TrafficControl {
    interface: String,
}

impl TrafficControl {
    pub fn new(interface: impl Into<String>) -> Self {
        Self {
            interface: interface.into(),
        }
    }

    pub fn interface(&self) -> &str {
        &self.interface
    }

    /// Helper function to run tc commands
    #[cfg(target_os = "linux")]
    async fn run_tc<I, S>(&self, args: I) -> Result<(), TcError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        use tokio::process::Command;

        let output = Command::new("tc").args(args).output().await?;

        if !output.status.success() {
            return Err(TcError::Tc(
                output.status,
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    async fn run_tc<I, S>(&self, _args: I) -> Result<(), TcError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Ok(())
    }

    /// Start building a new netem configuration
    pub fn netem(&self) -> NetemBuilder {
        NetemBuilder::new(self)
    }

    /// Remove all traffic control rules from the interface
    pub async fn reset(&self) -> Result<(), TcError> {
        self.run_tc(&["qdisc", "del", "dev", &self.interface, "root"])
            .await
    }

    /// Test latency to localhost using surge-ping
    pub async fn test_latency(&self, ip: IpAddr) -> Result<Duration, TcError> {
        let payload = [0; 8];
        let (_packet, duration) = surge_ping::ping(ip, &payload).await?;
        Ok(duration)
    }
}

#[derive(Default)]
struct NetemConfig {
    latency_ms: Option<u32>,
    loss_percent: Option<f32>,
}

pub struct NetemBuilder<'a> {
    tc: &'a TrafficControl,
    config: NetemConfig,
}

impl<'a> NetemBuilder<'a> {
    fn new(tc: &'a TrafficControl) -> Self {
        Self {
            tc,
            config: NetemConfig::default(),
        }
    }

    /// Add latency to the configuration
    pub fn latency(mut self, latency_ms: u32) -> Self {
        self.config.latency_ms = Some(latency_ms);
        self
    }

    /// Add packet loss to the configuration
    pub fn loss(mut self, loss_percent: f32) -> Self {
        self.config.loss_percent = Some(loss_percent);
        self
    }

    /// Apply the configuration
    pub async fn apply(self) -> Result<(), TcError> {
        let mut args: Vec<Cow<'static, OsStr>> = vec![
            OsStr::new("qdisc").into(),
            OsStr::new("add").into(),
            OsStr::new("dev").into(),
            OsString::from(&self.tc.interface).into(),
            OsStr::new("root").into(),
            OsStr::new("handle").into(),
            OsStr::new("1:0").into(),
            OsStr::new("netem").into(),
        ];

        if let Some(latency) = self.config.latency_ms {
            args.push(OsStr::new("delay").into());
            args.push(OsString::from(format!("{latency:.2}msec")).into());
        }

        if let Some(loss) = self.config.loss_percent {
            args.push(OsStr::new("loss").into());
            args.push(OsString::from(format!("{loss}%")).into());
        }

        self.tc.run_tc(args).await
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn latency_and_loss() -> Result<(), TcError> {
        let localhost = "127.0.0.1".parse().unwrap();

        let tc = TrafficControl::new("lo");

        // Test adding just latency
        tc.netem().latency(50).apply().await?;

        // Measure latency
        let latency = tc.test_latency(localhost).await?;
        println!("Measured latency: {latency:.3?}");

        if latency.as_millis() < 50 {
            panic!("Latency is too low");
        }

        // Clean up
        tc.reset().await?;

        Ok(())
    }

    #[tokio::test]
    async fn just_latency() -> Result<(), TcError> {
        let localhost = "127.0.0.1".parse().unwrap();

        let tc = TrafficControl::new("lo");

        // Test adding both latency and loss
        tc.netem().latency(50).loss(1.0).apply().await?;

        // Measure latency again
        let latency = tc.test_latency(localhost).await?;
        println!("Measured latency with loss: {latency:.3?}");

        if latency.as_millis() < 50 {
            panic!("Latency is too low");
        }

        // Clean up
        tc.reset().await?;

        Ok(())
    }

    #[tokio::test]
    async fn just_loss() -> Result<(), TcError> {
        let localhost = "127.0.0.1".parse().unwrap();

        let tc = TrafficControl::new("lo");

        // Test adding just packet loss
        tc.netem().loss(1.0).apply().await?;

        // Measure latency
        let latency = tc.test_latency(localhost).await?;
        println!("Measured latency with just loss: {latency:.3?}");

        if latency.as_millis() > 10 {
            panic!("Latency is too high");
        }

        // Clean up
        tc.reset().await?;

        Ok(())
    }
}
