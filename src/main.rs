use anyhow::{Context, Result};
use chrono::Local;
use config::{get_config, Config};
use mio::net::UdpSocket;
use mio::{Events, Interest, Poll, Token};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Error, ErrorKind, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::str;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};
use threadpool::ThreadPool;

mod config;

const SERVER: Token = Token(0);

struct CurrentFile {
    file: File,
    file_name: String,
}

// TODO: use Path for handling paths for safer joining of file names with directories
// BUG: It does not rotate what was an active log if the program is closed and re-opened

fn main() -> Result<(), Error> {
    #[cfg(target_os = "linux")]
    let config_paths = vec!["./config.toml", "/etc/syslogrs/config.toml"];

    #[cfg(target_os = "windows")]
    let config_paths = vec!["./config.toml", "/etc/syslogrs/config.toml"];

    let config = match get_config(config_paths) {
        Ok(config) => config,
        Err(e) => {
            println!("Error reading config: {:?}", e);
            println!("Using default config");
            Config::default()
        }
    };

    println!("Using the following config: {:#?}", config);

    let mut events = Events::with_capacity(256);
    let mut poll = Poll::new()?;
    let mut buffer = [0u8; 1024];

    // Open UDP socket using socket2
    let udp4_server_s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    // Set listening address (0.0.0.0:514)
    let sa4 = SocketAddr::new(Ipv4Addr::new(0, 0, 0, 0).into(), config.port);

    // Enable address reuse and bind to address
    udp4_server_s.set_reuse_address(true)?;
    //udp4_server_s.set_reuse_port(true)?; // Add for Unix CFG, does not work on Windows
    udp4_server_s.bind(&sa4.into())?;
    let mut udp4_server_mio = UdpSocket::from_std(udp4_server_s.into());

    // Register UDP socket with poll
    poll.registry()
        .register(&mut udp4_server_mio, SERVER, Interest::READABLE)?;

    let pool = ThreadPool::new(num_cpus::get());

    let source_logs: Arc<Mutex<HashMap<String, CurrentFile>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let log_writer = source_logs.clone();

    // Create a dedicated thread for log file management
    {
        let config = config.clone();
        thread::spawn(move || {
            let mut last_rotate_time = SystemTime::now();
            loop {
                // Check if it's time to rotate the log files
                if last_rotate_time.elapsed().unwrap().as_secs() >= config.log_rotate_interval {
                    match rotate_logs(&log_writer, &config) {
                        Ok(_) => {}
                        Err(e) => {
                            println!("Encountered error rotating logs: {:?}", e)
                        }
                    }
                    last_rotate_time = SystemTime::now();
                }
                // Check once per minute
                thread::sleep(Duration::from_secs(60));
            }
        });
    }

    let mut shutdown = false;
    while !shutdown {
        poll.poll(&mut events, None)?; // Poll for events
        for event in events.iter() {
            match event.token() {
                // Check for event token
                SERVER => match receive(&udp4_server_mio, &mut buffer, &source_logs, &config) {
                    // Receive UDP packet
                    Ok(()) => continue,
                    Err(e) => {
                        eprintln!("Receive error: {}", e);
                        shutdown = true; // Shutdown if error
                    }
                },
                _ => shutdown = true, // Shutdown if other token
            }
        }
    }

    pool.join();

    Ok(())
}

fn rotate_logs(
    log_writer: &Arc<Mutex<HashMap<String, CurrentFile>>>,
    config: &Config,
) -> Result<()> {
    let mut logs = log_writer.lock().unwrap();

    // Rename log files
    for (source, log_file) in logs.iter_mut() {
        match rename_logs_for_rotation(source, &log_file.file_name, config) {
            Ok(log_file_name) => {
                let mut new_log_file = create_log_file(&config.log_dir, source, config).unwrap();
                // Swap in new file object
                // This drops the log file and allows it to be opened by the archiver function
                std::mem::swap(log_file, &mut new_log_file);
                // Archive the old log file
                // Threading would be useful here to free up the writer lock quicker
                if config.compress {
                    archive_log_file(&log_file_name)
                        .context(format!("Error archiving log file: {}", log_file_name))?;
                }
                println!("Rotated log {}", log_file.file_name);
            }
            Err(e) => println!("Failed to rotate log {}: {:?}", log_file.file_name, e),
        }
    }
    Ok(())
}

fn rename_logs_for_rotation(
    source: &String,
    log_file_name: &str,
    config: &Config,
) -> Result<String> {
    let current_time = Local::now();
    let new_log_file_name = format!(
        "{}/syslog-{}_{}.log",
        config.log_dir,
        source,
        current_time.format("%Y-%m-%d-%H-%M-%S")
    );
    // Attempt to rename the log, if we succeed we can archive
    // If we fail, skip rotation for now
    std::fs::rename(log_file_name, new_log_file_name.clone())
        .context("Renaming log on rotation failed")?;

    // Create new log file
    Ok(new_log_file_name)
}

fn receive(
    sock: &UdpSocket,
    buf: &mut [u8],
    log_writer: &Arc<Mutex<HashMap<String, CurrentFile>>>,
    config: &Config,
) -> Result<(), Error> {
    loop {
        // Get info about the sender
        let (_len, from) = match sock.recv_from(buf) {
            Ok((len, from)) => (len, from), // Return length and sender
            Err(e) => {
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::Interrupted {
                    // If would block, return
                    return Ok(());
                } else {
                    return Err(e);
                }
            }
        };
        // Read the message
        let s = match str::from_utf8(&buf[.._len]) {
            Ok(v) => v,
            Err(e) => panic!("Not UTF-8: {}", e),
        };

        if config.display_stdout {
            println!("[{}] {}", from, s);
        }

        // Only use IP from source for classification purposes
        let source = from.ip().to_string();

        // Send the syslog message to the log manager thread for processing
        if config.store_logs {
            log_writer
                .lock()
                .unwrap()
                .entry(source.to_string())
                .or_insert_with(|| create_log_file(&config.log_dir, &source, config).unwrap())
                .file
                .write_all(format!("{}\n", s.trim()).as_bytes())?;
        }

        for elem in buf.iter_mut() {
            *elem = 0;
        } // Reset array to clear buffer
    }
}

fn create_log_file(log_dir: &String, source: &str, config: &Config) -> Result<CurrentFile, Error> {
    let log_file_name = format!("{}/syslog-{}.log", log_dir, source);

    if Path::new(log_file_name.as_str()).exists() {
        println!("{} already exists.  Attempting rotation", log_file_name);
        match rename_logs_for_rotation(&source.to_string(), &log_file_name.to_string(), config) {
            Ok(_) => println!("Succesfully rotated existing log file {}", log_file_name),
            Err(e) => {
                //We need to decide what to do here, try a different rename? use same log?
                println!("Failed to rename log: {:?}", e)
            }
        }
    }

    match File::create(log_file_name.clone()) {
        Ok(file) => {
            println!("Log created: {}", log_file_name);
            Ok(CurrentFile {
                file,
                file_name: log_file_name,
            })
        }
        Err(e) => Err(e),
    }
}

fn archive_log_file(log_file: &str) -> Result<(), Error> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    let log_file_path = log_file.to_string();
    let gz_file_path = format!("{}.gz", log_file_path);

    println!(
        "Compressing log file {} into {}",
        log_file_path, gz_file_path
    );

    let mut input = std::io::BufReader::new(File::open(&log_file_path)?);
    let output = File::create(gz_file_path.clone())?;
    let mut encoder = GzEncoder::new(output, Compression::default());
    let start = std::time::Instant::now();
    std::io::copy(&mut input, &mut encoder).unwrap();
    let output = encoder.finish().unwrap();
    println!(
        "Compression for {} completed in {:?}.  File size: {:?}",
        gz_file_path,
        start.elapsed(),
        output.metadata().unwrap().len()
    );

    //remove old log file
    std::fs::remove_file(std::path::Path::new(log_file_path.as_str()))?;

    Ok(())
}
