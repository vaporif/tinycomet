use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;

use bytes::{Buf, BufMut, BytesMut};
use prost::Message;
use tendermint_abci::Application;
use tendermint_proto::v0_38::abci::{Request, Response};

const READ_BUF_SIZE: usize = 1024 * 1024;

pub fn serve(listener: UnixListener, app: impl Application) {
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("accept error: {e}");
                continue;
            }
        };
        let app = app.clone();
        thread::spawn(move || handle_connection(stream, app));
    }
}

fn handle_connection(stream: UnixStream, app: impl Application) {
    tracing::debug!("new CometBFT connection");
    let mut reader = WireReader::new(&stream);
    let mut writer = WireWriter::new(&stream);

    while let Some(request) = reader.next_request() {
        let request = match request {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("failed to read request: {e}");
                return;
            }
        };
        let response = dispatch(&app, request);
        if let Err(e) = writer.send(&response) {
            tracing::error!("failed to send response: {e}");
            return;
        }
    }
    tracing::debug!("CometBFT connection closed");
}

fn dispatch(app: &impl Application, request: Request) -> Response {
    use tendermint_proto::v0_38::abci::{request::Value, response};

    let value = match request.value {
        Some(v) => v,
        None => return Response { value: None },
    };

    let response_value = match value {
        Value::Echo(req) => response::Value::Echo(app.echo(req)),
        Value::Flush(_) => response::Value::Flush(app.flush()),
        Value::Info(req) => response::Value::Info(app.info(req)),
        Value::InitChain(req) => response::Value::InitChain(app.init_chain(req)),
        Value::Query(req) => response::Value::Query(app.query(req)),
        Value::CheckTx(req) => response::Value::CheckTx(app.check_tx(req)),
        Value::Commit(_) => response::Value::Commit(app.commit()),
        Value::ListSnapshots(_) => response::Value::ListSnapshots(app.list_snapshots()),
        Value::OfferSnapshot(req) => response::Value::OfferSnapshot(app.offer_snapshot(req)),
        Value::LoadSnapshotChunk(req) => {
            response::Value::LoadSnapshotChunk(app.load_snapshot_chunk(req))
        }
        Value::ApplySnapshotChunk(req) => {
            response::Value::ApplySnapshotChunk(app.apply_snapshot_chunk(req))
        }
        Value::PrepareProposal(req) => response::Value::PrepareProposal(app.prepare_proposal(req)),
        Value::ProcessProposal(req) => response::Value::ProcessProposal(app.process_proposal(req)),
        Value::ExtendVote(req) => response::Value::ExtendVote(app.extend_vote(req)),
        Value::VerifyVoteExtension(req) => {
            response::Value::VerifyVoteExtension(app.verify_vote_extension(req))
        }
        Value::FinalizeBlock(req) => response::Value::FinalizeBlock(app.finalize_block(req)),
    };

    Response {
        value: Some(response_value),
    }
}

struct WireReader<'a> {
    stream: &'a UnixStream,
    buf: BytesMut,
    window: Vec<u8>,
}

impl<'a> WireReader<'a> {
    fn new(stream: &'a UnixStream) -> Self {
        Self {
            stream,
            buf: BytesMut::new(),
            window: vec![0u8; READ_BUF_SIZE],
        }
    }

    fn next_request(&mut self) -> Option<Result<Request, std::io::Error>> {
        use std::io::Read;
        loop {
            if let Some(msg) = self.try_decode() {
                return Some(msg);
            }
            let n = match self.stream.read(&mut self.window) {
                Ok(0) => return None,
                Ok(n) => n,
                Err(e) => return Some(Err(e)),
            };
            self.buf.extend_from_slice(&self.window[..n]);
        }
    }

    fn try_decode(&mut self) -> Option<Result<Request, std::io::Error>> {
        let mut probe = self.buf.clone().freeze();
        let len = match prost::encoding::decode_varint(&mut probe) {
            Ok(len) => len as usize,
            Err(_) if self.buf.len() <= 16 => return None,
            Err(e) => return Some(Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e))),
        };

        let header_len = self.buf.len() - probe.remaining();
        if probe.remaining() < len {
            return None;
        }

        self.buf.advance(header_len);
        let msg_bytes = self.buf.split_to(len);
        match Request::decode(&*msg_bytes) {
            Ok(req) => Some(Ok(req)),
            Err(e) => Some(Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e))),
        }
    }
}

struct WireWriter<'a> {
    stream: &'a UnixStream,
    buf: BytesMut,
}

impl<'a> WireWriter<'a> {
    fn new(stream: &'a UnixStream) -> Self {
        Self {
            stream,
            buf: BytesMut::new(),
        }
    }

    fn send(&mut self, response: &Response) -> Result<(), std::io::Error> {
        use std::io::Write;

        self.buf.clear();
        let mut msg_buf = BytesMut::new();
        response
            .encode(&mut msg_buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        prost::encoding::encode_varint(msg_buf.len() as u64, &mut self.buf);
        self.buf.put(msg_buf);

        let mut written = 0;
        while written < self.buf.len() {
            let n = self.stream.write(&self.buf[written..])?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "failed to write to stream",
                ));
            }
            written += n;
        }
        self.stream.flush()?;
        Ok(())
    }
}
