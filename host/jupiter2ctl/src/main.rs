use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

const TFTP_DATA_SIZE: usize = 512;
const TFTP_TIMEOUT: Duration = Duration::from_secs(3);
const TFTP_RETRIES: usize = 3;

#[derive(Parser, Debug)]
#[command(
    name = "jupiter2ctl",
    about = "Bring-up UART/TFTP de Exec Modernisé V4"
)]
struct Cli {
    #[command(subcommand)]
    command: CommandLine,
}

#[derive(Subcommand, Debug)]
enum CommandLine {
    /// Affiche les octets reçus sur l'UART DEBUG de la carte.
    Monitor {
        #[arg(long, default_value = "/dev/ttyUSB0")]
        serial: String,
        #[arg(long, default_value_t = 115_200)]
        baud: u32,
    },
    /// Répond aux requêtes RRQ TFTP du bootloader Jupiter 2.
    TftpServe {
        #[arg(long, default_value = "0.0.0.0:69")]
        bind: String,
        #[arg(long, default_value = "tftp-root")]
        root: PathBuf,
    },
    /// Affiche le contrat et les paramètres matériels de premier bring-up.
    Info,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        CommandLine::Monitor { serial, baud } => monitor(&serial, baud),
        CommandLine::TftpServe { bind, root } => tftp_serve(&bind, &root),
        CommandLine::Info => {
            println!("Exec Modernisé V4 — bring-up Milk-V Jupiter 2");
            println!("UART DEBUG : /dev/ttyUSB0, logique 1,8 V, 115200 8N1, sans contrôle de flux");
            println!("TFTP       : le bootloader doit envoyer une requête RRQ au serveur");
            println!(
                "Protocole  : version {}",
                exec_modernise_v4_protocol::PROTOCOL_VERSION
            );
            Ok(())
        }
    }
}

fn monitor(path: &str, baud: u32) -> Result<()> {
    let mut port = serialport::new(path, baud)
        .data_bits(serialport::DataBits::Eight)
        .stop_bits(serialport::StopBits::One)
        .parity(serialport::Parity::None)
        .flow_control(serialport::FlowControl::None)
        .timeout(Duration::from_millis(250))
        .open()
        .with_context(|| format!("ouverture UART {path} à {baud} bauds"))?;

    eprintln!("UART ouvert sur {path} à {baud} 8N1, Ctrl-C pour quitter");
    let mut buffer = [0_u8; 256];
    let stdout = io::stdout();
    let mut output = stdout.lock();
    loop {
        match port.read(&mut buffer) {
            Ok(count) => output.write_all(&buffer[..count])?,
            Err(error) if error.kind() == io::ErrorKind::TimedOut => {}
            Err(error) => return Err(error).context("lecture UART"),
        }
        output.flush()?;
    }
}

fn tftp_serve(bind: &str, root: &Path) -> Result<()> {
    let socket =
        UdpSocket::bind(bind).with_context(|| format!("liaison du serveur TFTP sur {bind}"))?;
    socket
        .set_read_timeout(Some(TFTP_TIMEOUT))
        .context("configuration du délai TFTP")?;
    println!("TFTP en écoute sur {bind}, racine {}", root.display());

    let mut packet = [0_u8; 2048];
    loop {
        let (size, peer) = match socket.recv_from(&mut packet) {
            Ok(result) => result,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error).context("réception TFTP"),
        };
        if let Some(request) = parse_rrq(&packet[..size])? {
            match request_file(root, &request.filename) {
                Ok(data) => {
                    println!(
                        "RRQ {} depuis {peer} ({} octets)",
                        request.filename,
                        data.len()
                    );
                    if let Err(error) = serve_file(&socket, peer, &data) {
                        eprintln!("transfert TFTP interrompu pour {peer}: {error:#}");
                    }
                }
                Err(error) => send_error(&socket, peer, 1, &error.to_string())?,
            }
        }
    }
}

struct ReadRequest {
    filename: String,
}

fn parse_rrq(packet: &[u8]) -> Result<Option<ReadRequest>> {
    if packet.len() < 4 || u16::from_be_bytes([packet[0], packet[1]]) != 1 {
        return Ok(None);
    }
    let fields = packet[2..]
        .split(|byte| *byte == 0)
        .take(2)
        .collect::<Vec<_>>();
    if fields.len() < 2 || fields[0].is_empty() {
        bail!("requête RRQ TFTP invalide");
    }
    let filename = std::str::from_utf8(fields[0])?.to_owned();
    let mode = std::str::from_utf8(fields[1])?;
    if !mode.eq_ignore_ascii_case("octet") {
        bail!("mode TFTP non supporté: {mode}");
    }
    Ok(Some(ReadRequest { filename }))
}

fn request_file(root: &Path, name: &str) -> Result<Vec<u8>> {
    let relative = Path::new(name);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        bail!("nom de fichier TFTP refusé");
    }
    let path = root.join(relative);
    let mut file = File::open(&path).with_context(|| format!("ouverture de {}", path.display()))?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    Ok(data)
}

fn serve_file(socket: &UdpSocket, peer: SocketAddr, data: &[u8]) -> Result<()> {
    let mut block: u16 = 1;
    let mut offset = 0;
    loop {
        let end = (offset + TFTP_DATA_SIZE).min(data.len());
        let chunk = &data[offset..end];
        let mut packet = Vec::with_capacity(4 + chunk.len());
        packet.extend_from_slice(&3_u16.to_be_bytes());
        packet.extend_from_slice(&block.to_be_bytes());
        packet.extend_from_slice(chunk);
        send_with_ack(socket, peer, &packet, block)?;
        offset = end;
        if chunk.len() < TFTP_DATA_SIZE {
            break;
        }
        block = block.wrapping_add(1);
    }
    Ok(())
}

fn send_with_ack(socket: &UdpSocket, peer: SocketAddr, packet: &[u8], block: u16) -> Result<()> {
    let mut response = [0_u8; 516];
    for _ in 0..TFTP_RETRIES {
        socket.send_to(packet, peer)?;
        match socket.recv_from(&mut response) {
            Ok((size, from)) if from == peer && size >= 4 => {
                let opcode = u16::from_be_bytes([response[0], response[1]]);
                let acknowledged = u16::from_be_bytes([response[2], response[3]]);
                if opcode == 4 && acknowledged == block {
                    return Ok(());
                }
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(error).context("réception ACK TFTP"),
        }
    }
    bail!("aucun ACK TFTP pour le bloc {block}")
}

fn send_error(socket: &UdpSocket, peer: SocketAddr, code: u16, message: &str) -> Result<()> {
    let mut packet = Vec::new();
    packet.extend_from_slice(&5_u16.to_be_bytes());
    packet.extend_from_slice(&code.to_be_bytes());
    packet.extend_from_slice(message.as_bytes());
    packet.push(0);
    socket.send_to(&packet, peer)?;
    Ok(())
}
