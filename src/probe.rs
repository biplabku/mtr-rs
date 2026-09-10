use std::net::IpAddr;
use std::time::Duration;

use anyhow::Result;
use multiprobe::Probe;
use tokio::sync::mpsc;

pub struct HopStats {
    pub ttl: u8,
    pub addr: Option<IpAddr>,
    pub sent: u32,
    pub received: u32,
    pub last_rtt: f64,
    pub best_rtt: f64,
    pub worst_rtt: f64,
    pub total_rtt: f64,
    pub rtt_history: Vec<f64>,
}

impl HopStats {
    pub fn new(ttl: u8) -> Self {
        Self {
            ttl,
            addr: None,
            sent: 0,
            received: 0,
            last_rtt: 0.0,
            best_rtt: f64::MAX,
            worst_rtt: 0.0,
            total_rtt: 0.0,
            rtt_history: Vec::with_capacity(64),
        }
    }

    pub fn update(&mut self, rtt_ms: f64, success: bool) {
        self.sent += 1;

        if success && rtt_ms > 0.0 {
            self.received += 1;
            self.last_rtt = rtt_ms;
            self.total_rtt += rtt_ms;

            if rtt_ms < self.best_rtt {
                self.best_rtt = rtt_ms;
            }
            if rtt_ms > self.worst_rtt {
                self.worst_rtt = rtt_ms;
            }

            self.rtt_history.push(rtt_ms);
            if self.rtt_history.len() > 64 {
                self.rtt_history.remove(0);
            }
        }
    }

    pub fn loss_percent(&self) -> f64 {
        if self.sent == 0 {
            0.0
        } else {
            ((self.sent - self.received) as f64 / self.sent as f64) * 100.0
        }
    }

    pub fn avg_rtt(&self) -> f64 {
        if self.received == 0 {
            0.0
        } else {
            self.total_rtt / self.received as f64
        }
    }

    pub fn jitter(&self) -> f64 {
        if self.rtt_history.len() < 2 {
            return 0.0;
        }

        let mut total_diff = 0.0;
        for i in 1..self.rtt_history.len() {
            total_diff += (self.rtt_history[i] - self.rtt_history[i - 1]).abs();
        }
        total_diff / (self.rtt_history.len() - 1) as f64
    }

    #[allow(dead_code)]
    pub fn sparkline_data(&self) -> Vec<u64> {
        if self.rtt_history.is_empty() {
            return vec![0];
        }

        let max_val = self.rtt_history.iter().cloned().fold(0.0_f64, f64::max);
        if max_val == 0.0 {
            return vec![0; self.rtt_history.len()];
        }

        self.rtt_history
            .iter()
            .map(|&v| ((v / max_val) * 8.0) as u64)
            .collect()
    }
}

struct HopData {
    ttl: u8,
    addr: Option<IpAddr>,
    rtt: Duration,
}

pub struct ProbeEngine {
    target: String,
    max_hops: u8,
    timeout: Duration,
    paris_mode: bool,
}

impl ProbeEngine {
    pub fn new(target: String, max_hops: u8, timeout: Duration, paris_mode: bool) -> Self {
        Self {
            target,
            max_hops,
            timeout,
            paris_mode,
        }
    }

    async fn trace(&self) -> Result<Vec<HopData>> {
        if self.paris_mode {
            let trace = Probe::paris(&self.target)
                .max_hops(self.max_hops)
                .timeout_per_hop(self.timeout)
                .send()
                .await?;

            Ok(trace
                .hops
                .iter()
                .map(|h| HopData {
                    ttl: h.ttl as u8,
                    addr: h.addr,
                    rtt: h.rtt,
                })
                .collect())
        } else {
            let trace = Probe::traceroute(&self.target)
                .max_hops(self.max_hops)
                .timeout_per_hop(self.timeout)
                .send()
                .await?;

            Ok(trace
                .hops
                .iter()
                .map(|h| HopData {
                    ttl: h.ttl as u8,
                    addr: h.addr,
                    rtt: h.rtt,
                })
                .collect())
        }
    }

    pub async fn trace_once(&self) -> Result<Vec<HopStats>> {
        let hops_data = self.trace().await?;

        let mut hops = Vec::new();
        for h in hops_data {
            let mut stats = HopStats::new(h.ttl);
            stats.addr = h.addr;
            if h.addr.is_some() {
                let rtt_ms = h.rtt.as_secs_f64() * 1000.0;
                stats.update(rtt_ms, true);
            } else {
                stats.update(0.0, false);
            }
            hops.push(stats);
        }

        Ok(hops)
    }

    pub async fn run_continuous(
        &self,
        tx: mpsc::Sender<(u8, Option<IpAddr>, f64, bool)>,
        interval: Duration,
        count_limit: u32,
    ) {
        let mut cycle = 0u32;

        loop {
            if count_limit > 0 && cycle >= count_limit {
                break;
            }

            match self.trace().await {
                Ok(hops_data) => {
                    for h in hops_data {
                        let rtt_ms = h.rtt.as_secs_f64() * 1000.0;
                        let success = h.addr.is_some();
                        let _ = tx.send((h.ttl, h.addr, rtt_ms, success)).await;
                    }
                }
                Err(_) => {
                    // Silently continue on errors
                }
            }

            // Signal end of cycle (ttl=0 is special marker)
            let _ = tx.send((0, None, 0.0, false)).await;
            cycle += 1;

            tokio::time::sleep(interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HopStats;

    #[test]
    fn test_hop_stats_new() {
        let hop = HopStats::new(1);
        assert_eq!(hop.ttl, 1);
        assert_eq!(hop.sent, 0);
        assert_eq!(hop.received, 0);
        assert_eq!(hop.last_rtt, 0.0);
        assert_eq!(hop.best_rtt, f64::MAX);
        assert_eq!(hop.worst_rtt, 0.0);
    }

    #[test]
    fn test_hop_stats_update_success() {
        let mut hop = HopStats::new(1);

        hop.update(10.5, true);
        assert_eq!(hop.sent, 1);
        assert_eq!(hop.received, 1);
        assert_eq!(hop.last_rtt, 10.5);
        assert_eq!(hop.best_rtt, 10.5);
        assert_eq!(hop.worst_rtt, 10.5);

        hop.update(5.0, true);
        assert_eq!(hop.sent, 2);
        assert_eq!(hop.received, 2);
        assert_eq!(hop.last_rtt, 5.0);
        assert_eq!(hop.best_rtt, 5.0);
        assert_eq!(hop.worst_rtt, 10.5);

        hop.update(20.0, true);
        assert_eq!(hop.sent, 3);
        assert_eq!(hop.received, 3);
        assert_eq!(hop.best_rtt, 5.0);
        assert_eq!(hop.worst_rtt, 20.0);
    }

    #[test]
    fn test_hop_stats_update_failure() {
        let mut hop = HopStats::new(1);

        hop.update(0.0, false);
        assert_eq!(hop.sent, 1);
        assert_eq!(hop.received, 0);
        assert_eq!(hop.last_rtt, 0.0);
        assert_eq!(hop.best_rtt, f64::MAX);
    }

    #[test]
    fn test_loss_percent() {
        let mut hop = HopStats::new(1);

        assert_eq!(hop.loss_percent(), 0.0);

        hop.update(10.0, true);
        assert_eq!(hop.loss_percent(), 0.0);

        hop.update(0.0, false);
        assert_eq!(hop.loss_percent(), 50.0);

        hop.update(0.0, false);
        hop.update(0.0, false);
        assert!((hop.loss_percent() - 75.0).abs() < 0.01);
    }

    #[test]
    fn test_avg_rtt() {
        let mut hop = HopStats::new(1);

        assert_eq!(hop.avg_rtt(), 0.0);

        hop.update(10.0, true);
        assert_eq!(hop.avg_rtt(), 10.0);

        hop.update(20.0, true);
        assert_eq!(hop.avg_rtt(), 15.0);

        hop.update(30.0, true);
        assert_eq!(hop.avg_rtt(), 20.0);
    }

    #[test]
    fn test_jitter() {
        let mut hop = HopStats::new(1);

        assert_eq!(hop.jitter(), 0.0);

        hop.update(10.0, true);
        assert_eq!(hop.jitter(), 0.0);

        hop.update(10.0, true);
        assert_eq!(hop.jitter(), 0.0);

        hop.update(20.0, true);
        assert_eq!(hop.jitter(), 5.0);
    }

    #[test]
    fn test_jitter_with_variance() {
        let mut hop = HopStats::new(1);

        hop.update(10.0, true);
        hop.update(15.0, true);
        hop.update(12.0, true);
        hop.update(18.0, true);

        // Diffs: |15-10|=5, |12-15|=3, |18-12|=6 → total=14, avg=14/3≈4.67
        let jitter = hop.jitter();
        assert!(jitter > 0.0);
        assert!((jitter - 4.67).abs() < 0.1);
    }

    #[test]
    fn test_sparkline_data_empty() {
        let hop = HopStats::new(1);
        assert_eq!(hop.sparkline_data(), vec![0]);
    }

    #[test]
    fn test_sparkline_data_with_values() {
        let mut hop = HopStats::new(1);

        hop.update(10.0, true);
        hop.update(20.0, true);
        hop.update(15.0, true);

        let data = hop.sparkline_data();
        assert_eq!(data.len(), 3);
        assert!(data[1] == 8);
    }

    #[test]
    fn test_rtt_history_cap() {
        let mut hop = HopStats::new(1);

        for i in 0..100 {
            hop.update(i as f64, true);
        }

        assert_eq!(hop.rtt_history.len(), 64);
        assert_eq!(hop.rtt_history[0], 36.0);
        assert_eq!(hop.rtt_history[63], 99.0);
    }

    #[test]
    fn test_mixed_success_failure() {
        let mut hop = HopStats::new(5);

        hop.update(10.0, true);
        hop.update(0.0, false);
        hop.update(15.0, true);
        hop.update(0.0, false);
        hop.update(20.0, true);

        assert_eq!(hop.sent, 5);
        assert_eq!(hop.received, 3);
        assert_eq!(hop.loss_percent(), 40.0);
        assert_eq!(hop.best_rtt, 10.0);
        assert_eq!(hop.worst_rtt, 20.0);
        assert_eq!(hop.avg_rtt(), 15.0);
    }
}
