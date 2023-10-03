use chrono::Local;
use mio::net::UdpSocket;
use mio::{Events, Interest, Poll, Token};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::io::{Error, ErrorKind, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::str;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};
use threadpool::ThreadPool;
use zip::write::FileOptions;
use zip::ZipWriter;

const SYSLOG_UDP_PORT: u16 = 514;
const SERVER: Token = Token(0);
const LOG_ROTATE_INTERVAL: u64 = 3600;
const LOG_DIR: &str = "./";

struct CurrentFile {
    file: File,
    file_name: String,
}

fn main() -> Result<(), Error> {
    let mut events = Events::with_capacity(256);
    let mut poll = Poll::new()?;
    let mut buffer = [0u8; 1024];

    // Open UDP socket using socket2
    let udp4_server_s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    // Set listening address (0.0.0.0:514)
    let sa4 = SocketAddr::new(Ipv4Addr::new(0, 0, 0, 0).into(), SYSLOG_UDP_PORT);

    // Enable address reuse and bind to address
    udp4_server_s.set_reuse_address(true)?;
    #[cfg(unix)]
    udp4_server_s.set_reuse_port(true)?; // Add for Unix CFG, does not work on Windows
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
    thread::spawn(move || {
        let mut last_rotate_time = SystemTime::now();
        loop {
            // Check if it's time to rotate the log files
            if last_rotate_time.elapsed().unwrap().as_secs() >= LOG_ROTATE_INTERVAL {
                let mut logs = log_writer.lock().unwrap();

                for (source, log_file) in logs.iter_mut() {
                    let mut new_log_file = create_log_file(source).unwrap();

                    let log_file_name = log_file.file_name.to_string();
                    std::mem::swap(log_file, &mut new_log_file);
                    let zip_file_name = format!("{}.zip", log_file_name);

                    if let Err(err) = archive_log_file(&log_file_name, &zip_file_name) {
                        eprintln!("Error archiving log file: {}", err);
                    }
                }

                last_rotate_time = SystemTime::now();
            }
            // Check once per minute
            thread::sleep(Duration::from_secs(60));
        }
    });

    let mut shutdown = false;
    while !shutdown {
        poll.poll(&mut events, None)?; // Poll for events
        for event in events.iter() {
            match event.token() {
                // Check for event token
                SERVER => match receive(&udp4_server_mio, &mut buffer, &source_logs) {
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

fn receive(
    sock: &UdpSocket,
    buf: &mut [u8],
    log_writer: &Arc<Mutex<HashMap<String, CurrentFile>>>,
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

        println!("[{}] {}", from, s);

        let source = from.to_string();
        // TODO: regex out IP address
        let source = source.replace(":514", "");

        // Send the syslog message to the log manager thread for processing
        log_writer
            .lock()
            .unwrap()
            .entry(source.to_string())
            .or_insert_with(|| create_log_file(&source).unwrap())
            .file
            .write(format!("{}\n", s.trim()).as_bytes())?;

        for elem in buf.iter_mut() {
            *elem = 0;
        } // Reset array to clear buffer
    }
}

fn create_log_file(source: &str) -> Result<CurrentFile, Error> {
    let current_time = Local::now();
    let log_file_name = format!(
        "{}/syslog-{}-{}.log",
        LOG_DIR,
        source,
        current_time.format("%Y-%m-%d-%H-%M-%S")
    );
    println!("Log created: {}", log_file_name);

    match File::create(log_file_name.clone()) {
        Ok(file) => Ok(CurrentFile {
            file: file,
            file_name: log_file_name,
        }),
        Err(e) => Err(e),
    }
}

fn archive_log_file(log_file_path: &str, zip_file_path: &str) -> Result<(), Error> {
    println!("Putting log {} into {}", log_file_path, zip_file_path);

    let file = File::open(log_file_path)?;
    let zip_file = File::create(zip_file_path)?;

    let mut zip_writer = ZipWriter::new(zip_file);
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip_writer.start_file("log.txt", options)?;

    std::io::copy(&mut file.take(u64::MAX), &mut zip_writer)?;

    //remove old log file
    std::fs::remove_file(std::path::Path::new(log_file_path))?;

    Ok(())
}
