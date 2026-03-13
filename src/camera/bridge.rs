use crate::camera::protocol::{AdapterCommand, AdapterMessage, AdapterSourceInfo};
use crate::camera::{CameraDescriptor, CameraOrigin, FramePacket};
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use crossbeam_channel::{bounded, Receiver, Sender};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    thread::{self, JoinHandle},
};

const PROTOCOL_VERSION: u32 = 1;

pub struct AdapterCameraStream {
    rx: Receiver<FramePacket>,
    child: Option<Child>,
    join_handle: Option<JoinHandle<()>>,
}

impl AdapterCameraStream {
    pub fn try_recv(&self) -> Option<FramePacket> {
        let mut latest = None;
        while let Ok(frame) = self.rx.try_recv() {
            latest = Some(frame);
        }
        latest
    }

    pub fn stop(mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn probe_adapter(command_line: &str) -> Result<Vec<CameraDescriptor>> {
    let mut child = spawn_adapter(command_line)?;
    let stdin = child.stdin.as_mut().context("missing adapter stdin")?;
    send_command(
        stdin,
        &AdapterCommand::Hello {
            protocol_version: PROTOCOL_VERSION,
        },
    )?;
    send_command(stdin, &AdapterCommand::ListSources)?;

    let stdout = child.stdout.take().context("missing adapter stdout")?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let mut saw_hello = false;
    let mut result = None;

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let message: AdapterMessage = serde_json::from_str(line.trim())?;
        match message {
            AdapterMessage::Hello { .. } => saw_hello = true,
            AdapterMessage::Sources { sources } if saw_hello => {
                result = Some(
                    sources
                        .into_iter()
                        .map(|source| descriptor_from_adapter(command_line, source))
                        .collect::<Vec<_>>(),
                );
                break;
            }
            AdapterMessage::Error { message } => bail!("{message}"),
            _ => {}
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    result.ok_or_else(|| anyhow!("adapter did not return sources"))
}

pub fn open_adapter_camera(
    command_line: &str,
    source: &CameraDescriptor,
) -> Result<AdapterCameraStream> {
    let adapter_source_id = source
        .id
        .split_once("::")
        .map(|(_, source_id)| source_id.to_owned())
        .ok_or_else(|| anyhow!("invalid adapter source id"))?;

    let mut child = spawn_adapter(command_line)?;
    {
        let stdin = child.stdin.as_mut().context("missing adapter stdin")?;
        send_command(
            stdin,
            &AdapterCommand::Hello {
                protocol_version: PROTOCOL_VERSION,
            },
        )?;
        send_command(
            stdin,
            &AdapterCommand::Open {
                source_id: adapter_source_id,
            },
        )?;
        send_command(stdin, &AdapterCommand::Start)?;
    }

    let stdout = child.stdout.take().context("missing adapter stdout")?;
    let (tx, rx) = bounded(2);
    let join_handle = thread::spawn(move || {
        if let Err(error) = adapter_reader_loop(stdout, tx) {
            eprintln!("adapter stream ended: {error:?}");
        }
    });

    Ok(AdapterCameraStream {
        rx,
        child: Some(child),
        join_handle: Some(join_handle),
    })
}

fn spawn_adapter(command_line: &str) -> Result<Child> {
    let args = shlex::split(command_line).ok_or_else(|| anyhow!("invalid adapter command line"))?;
    let (program, args) = args
        .split_first()
        .ok_or_else(|| anyhow!("adapter command is empty"))?;
    Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to launch adapter: {command_line}"))
}

fn send_command(stdin: &mut ChildStdin, command: &AdapterCommand) -> Result<()> {
    let line = serde_json::to_string(command)?;
    stdin.write_all(line.as_bytes())?;
    stdin.write_all(b"\n")?;
    stdin.flush()?;
    Ok(())
}

fn adapter_reader_loop(stdout: impl std::io::Read, tx: Sender<FramePacket>) -> Result<()> {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();

    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }

        let message: AdapterMessage = serde_json::from_str(line.trim())?;
        if let AdapterMessage::Frame { frame } = message {
            if let Some(packet) = frame_to_packet(frame) {
                let _ = tx.try_send(packet);
            }
        }
    }

    Ok(())
}

fn frame_to_packet(frame: crate::camera::protocol::AdapterFrame) -> Option<FramePacket> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(frame.data_base64.as_bytes())
        .ok()?;

    Some(FramePacket {
        width: frame.width,
        height: frame.height,
        pixel_format: frame.pixel_format,
        sequence: frame.sequence,
        source_name: "adapter".to_owned(),
        timestamp_millis: frame.timestamp_millis,
        data,
    })
}

fn descriptor_from_adapter(command_line: &str, source: AdapterSourceInfo) -> CameraDescriptor {
    CameraDescriptor {
        id: format!("adapter:{}::{id}", source.id, id = source.id),
        name: format!("{} ({})", source.name, command_line),
        width: source.width,
        height: source.height,
        nominal_fps: source.nominal_fps,
        origin: CameraOrigin::Adapter {
            command: command_line.to_owned(),
        },
    }
}
