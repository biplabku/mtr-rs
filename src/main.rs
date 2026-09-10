use std::io;
use std::net::IpAddr;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table},
    Frame, Terminal,
};
use tokio::sync::mpsc;

mod probe;

use probe::{HopStats, ProbeEngine};

#[derive(Parser, Debug)]
#[command(name = "mtr-rs")]
#[command(author = "Biplab Das")]
#[command(version = "0.1.0")]
#[command(about = "A modern network diagnostic tool powered by multiprobe", long_about = None)]
struct Args {
    /// Target hostname or IP address
    target: String,

    /// Maximum number of hops
    #[arg(short = 'm', long, default_value = "30")]
    max_hops: u8,

    /// Timeout per hop in seconds
    #[arg(short = 't', long, default_value = "2")]
    timeout: u64,

    /// Interval between probe cycles in milliseconds
    #[arg(short = 'i', long, default_value = "1000")]
    interval: u64,

    /// Use Paris Traceroute mode (ECMP-aware)
    #[arg(long)]
    paris: bool,

    /// Output in JSON format (non-interactive)
    #[arg(long)]
    json: bool,

    /// Number of probe cycles (0 = unlimited)
    #[arg(short = 'c', long, default_value = "0")]
    count: u32,
}

struct App {
    target: String,
    hops: Vec<HopStats>,
    probe_count: u32,
    running: bool,
    paris_mode: bool,
}

impl App {
    fn new(target: String, paris_mode: bool) -> Self {
        Self {
            target,
            hops: Vec::new(),
            probe_count: 0,
            running: true,
            paris_mode,
        }
    }

    fn update_hop(&mut self, ttl: u8, addr: Option<IpAddr>, rtt_ms: f64, success: bool) {
        let idx = (ttl - 1) as usize;

        while self.hops.len() <= idx {
            self.hops.push(HopStats::new(self.hops.len() as u8 + 1));
        }

        let hop = &mut self.hops[idx];
        hop.addr = addr;
        hop.update(rtt_ms, success);
    }

    fn increment_probe_count(&mut self) {
        self.probe_count += 1;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if args.json {
        run_json_mode(&args).await
    } else {
        run_tui_mode(&args).await
    }
}

async fn run_json_mode(args: &Args) -> Result<()> {
    let engine = ProbeEngine::new(
        args.target.clone(),
        args.max_hops,
        Duration::from_secs(args.timeout),
        args.paris,
    );

    let hops = engine.trace_once().await?;

    let output: Vec<_> = hops
        .iter()
        .map(|h| {
            let best = if h.best_rtt == f64::MAX { 0.0 } else { h.best_rtt };
            serde_json::json!({
                "ttl": h.ttl,
                "addr": h.addr.map(|a| a.to_string()),
                "sent": h.sent,
                "received": h.received,
                "loss_pct": h.loss_percent(),
                "last_ms": h.last_rtt,
                "avg_ms": h.avg_rtt(),
                "best_ms": best,
                "worst_ms": h.worst_rtt,
                "jitter_ms": h.jitter(),
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

async fn run_tui_mode(args: &Args) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(args.target.clone(), args.paris);

    let engine = ProbeEngine::new(
        args.target.clone(),
        args.max_hops,
        Duration::from_secs(args.timeout),
        args.paris,
    );

    let (tx, mut rx) = mpsc::channel::<(u8, Option<IpAddr>, f64, bool)>(100);

    let interval = Duration::from_millis(args.interval);
    let count_limit = args.count;

    tokio::spawn(async move {
        engine.run_continuous(tx, interval, count_limit).await;
    });

    loop {
        while let Ok((ttl, addr, rtt_ms, success)) = rx.try_recv() {
            if ttl == 0 {
                app.increment_probe_count();
            } else {
                app.update_hop(ttl, addr, rtt_ms, success);
            }
        }

        terminal.draw(|f| ui(f, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.running = false;
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }

        if !app.running {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(f.area());

    let mode_str = if app.paris_mode { "Paris" } else { "Standard" };
    let title = format!(
        " mtr-rs: {} | Mode: {} | Probes: {} | Press 'q' to quit ",
        app.target, mode_str, app.probe_count
    );

    let header_block = Block::default()
        .borders(Borders::ALL)
        .title(title);
    f.render_widget(header_block, chunks[0]);

    let header_cells = ["#", "Host", "Loss%", "Sent", "Recv", "Last", "Avg", "Best", "Wrst", "Jitter"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1);

    let rows = app.hops.iter().map(|hop| {
        let addr_str = hop.addr
            .map(|a| a.to_string())
            .unwrap_or_else(|| "???".to_string());

        let loss_color = if hop.loss_percent() > 50.0 {
            Color::Red
        } else if hop.loss_percent() > 10.0 {
            Color::Yellow
        } else {
            Color::Green
        };

        let best = if hop.best_rtt == f64::MAX { 0.0 } else { hop.best_rtt };

        let cells = vec![
            Cell::from(format!("{:2}", hop.ttl)),
            Cell::from(addr_str),
            Cell::from(format!("{:5.1}%", hop.loss_percent())).style(Style::default().fg(loss_color)),
            Cell::from(format!("{:4}", hop.sent)),
            Cell::from(format!("{:4}", hop.received)),
            Cell::from(format!("{:7.2}", hop.last_rtt)),
            Cell::from(format!("{:7.2}", hop.avg_rtt())),
            Cell::from(format!("{:7.2}", best)),
            Cell::from(format!("{:7.2}", hop.worst_rtt)),
            Cell::from(format!("{:7.2}", hop.jitter())),
        ];
        Row::new(cells)
    });

    let widths = [
        Constraint::Length(3),   // #
        Constraint::Min(20),     // Host
        Constraint::Length(7),   // Loss%
        Constraint::Length(5),   // Sent
        Constraint::Length(5),   // Recv
        Constraint::Length(8),   // Last
        Constraint::Length(8),   // Avg
        Constraint::Length(8),   // Best
        Constraint::Length(8),   // Wrst
        Constraint::Length(8),   // Jitter
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Hops "));

    f.render_widget(table, chunks[1]);
}
