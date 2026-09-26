use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use futures::StreamExt;
use prost::Message;
use solami::geyser::geyser_client::GeyserClient;
use solami::geyser::subscribe_update::UpdateOneof;
use solami::geyser::{
    CommitmentLevel, SlotStatus, SubscribeReplayInfoRequest, SubscribeRequest,
    SubscribeRequestPing, SubscribeUpdate,
};
use solami::{SubscribeRequestFilterSlots, SubscriptionBuilder, TxFilter};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tonic::codec::CompressionEncoding;
use tonic::transport::{Channel, ClientTlsConfig};
use tonic::{Status, Streaming};

use crate::api::Api;
use crate::env::Env;
use crate::rpc::Rpc;

/// Successful, non-vote transactions touching `program`, plus every slot status update.
pub fn request(
    program: &str,
    commitment: CommitmentLevel,
    from_slot: Option<u64>,
) -> SubscribeRequest {
    let mut builder = SubscriptionBuilder::new()
        .commitment(commitment)
        .transactions(
            "program",
            TxFilter {
                vote: Some(false),
                failed: Some(false),
                account_include: vec![program.to_owned()],
                account_exclude: vec![],
                account_required: vec![],
                signature: None,
            },
        )
        .slots(
            "slots",
            SubscribeRequestFilterSlots {
                filter_by_commitment: Some(false),
                interslot_updates: Some(false),
            },
        );
    if let Some(slot) = from_slot {
        builder = builder.from_slot(slot);
    }
    builder.build()
}

/// One open subscription. Answers server pings (when `pong` is set) so the caller doesn't have to.
pub struct Session {
    sink: mpsc::Sender<SubscribeRequest>,
    updates: Streaming<SubscribeUpdate>,
    pong: bool,
    pub pings: u64,
}

impl Session {
    pub async fn open(env: &Env, req: SubscribeRequest, pong: bool) -> Result<Self> {
        let mut client = solami::grpc::connect_public(&env.api_key, &env.grpc_url).await?;
        let (sink, updates) = client.subscribe(req).await?;
        Ok(Self {
            sink,
            updates,
            pong,
            pings: 0,
        })
    }

    /// Same subscription over our own tonic client, asking the server for zstd-compressed
    /// responses (the SDK's client doesn't negotiate compression).
    pub async fn open_zstd(env: &Env, req: SubscribeRequest, pong: bool) -> Result<Self> {
        let channel = Channel::from_shared(env.grpc_url.clone())?
            .tls_config(ClientTlsConfig::new().with_native_roots())?
            .connect_timeout(Duration::from_secs(10))
            .http2_keep_alive_interval(Duration::from_secs(10))
            .keep_alive_timeout(Duration::from_secs(20))
            .keep_alive_while_idle(true)
            .connect()
            .await?;
        let mut client = GeyserClient::new(channel)
            .accept_compressed(CompressionEncoding::Zstd)
            .max_decoding_message_size(64 * 1024 * 1024);
        let (sink, rx) = mpsc::channel(32);
        sink.send(req).await?;
        let mut request = tonic::Request::new(ReceiverStream::new(rx));
        request
            .metadata_mut()
            .insert("x-token", env.api_key.parse()?);
        let updates = client.subscribe(request).await?.into_inner();
        Ok(Self {
            sink,
            updates,
            pong,
            pings: 0,
        })
    }

    pub async fn next(&mut self) -> Option<Result<SubscribeUpdate, Status>> {
        let item = self.updates.next().await?;
        if let Ok(update) = &item
            && matches!(update.update_oneof, Some(UpdateOneof::Ping(_)))
        {
            self.pings += 1;
            if self.pong {
                let ping = SubscribeRequest {
                    ping: Some(SubscribeRequestPing { id: 1 }),
                    ..Default::default()
                };
                let _ = self.sink.send(ping).await;
            }
        }
        Some(item)
    }
}

pub fn describe(status: &Status) -> String {
    format!("{:?}: {}", status.code(), status.message())
}

fn now_nanos() -> i128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as i128
}

/// Milliseconds between the server stamping the update and us receiving it.
fn latency_ms(update: &SubscribeUpdate) -> Option<f64> {
    let t = update.created_at.as_ref()?;
    let created = t.seconds as i128 * 1_000_000_000 + t.nanos as i128;
    Some((now_nanos() - created) as f64 / 1e6)
}

fn tx_of(update: &SubscribeUpdate) -> Option<(u64, String)> {
    match &update.update_oneof {
        Some(UpdateOneof::Transaction(tx)) => {
            let sig = tx
                .transaction
                .as_ref()
                .map(|t| bs58::encode(&t.signature).into_string())?;
            Some((tx.slot, sig))
        }
        _ => None,
    }
}

fn slot_status(update: &SubscribeUpdate) -> Option<(u64, SlotStatus)> {
    match &update.update_oneof {
        Some(UpdateOneof::Slot(s)) => Some((s.slot, s.status())),
        _ => None,
    }
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i]
}

fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(|a, b| a.total_cmp(b));
    v
}

/// Spike 1: live stream rate, latency, slot statuses, and whether answering pings is safe.
pub async fn stream(env: &Env, secs: u64, commitment: CommitmentLevel, pong: bool) -> Result<()> {
    println!(
        "Subscribing to {} at {:?} for {secs}s (answer pings: {pong})",
        env.program, commitment
    );
    let t0 = Instant::now();
    let mut s = Session::open(env, request(&env.program, commitment, None), pong).await?;
    println!("subscribe accepted after {} ms", t0.elapsed().as_millis());

    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.tick().await;

    let mut first_update = None;
    let (mut txs, mut sec_txs, mut dups, mut txs_after_ping) = (0u64, 0u64, 0u64, 0u64);
    let mut sigs = HashSet::new();
    let mut lat = Vec::new();
    let mut statuses: BTreeMap<String, u64> = BTreeMap::new();
    let (mut min_slot, mut max_slot) = (u64::MAX, 0u64);
    // Slots whose SlotProcessed update has arrived; a transaction for one of these is "late".
    let mut processed = HashSet::new();
    let (mut late, mut steady_lat) = (0u64, Vec::new());
    let end = loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break "time limit reached".to_owned(),
            _ = tick.tick() => {
                println!("  t+{:>3}s  {:>4} tx/s  max tx slot {max_slot}  pings {}", t0.elapsed().as_secs(), sec_txs, s.pings);
                sec_txs = 0;
            }
            item = s.next() => match item {
                None => break "server closed the stream".to_owned(),
                Some(Err(st)) => break format!("stream error {}", describe(&st)),
                Some(Ok(u)) => {
                    first_update.get_or_insert(t0.elapsed());
                    if let Some((slot, sig)) = tx_of(&u) {
                        txs += 1;
                        sec_txs += 1;
                        if s.pings > 0 { txs_after_ping += 1; }
                        if !sigs.insert(sig) { dups += 1; }
                        min_slot = min_slot.min(slot);
                        max_slot = max_slot.max(slot);
                        if processed.contains(&slot) { late += 1; }
                        if let Some(ms) = latency_ms(&u) {
                            lat.push(ms);
                            if t0.elapsed() > Duration::from_secs(10) { steady_lat.push(ms); }
                        }
                    } else if let Some((slot, status)) = slot_status(&u) {
                        if status == SlotStatus::SlotProcessed { processed.insert(slot); }
                        *statuses.entry(format!("{status:?}")).or_default() += 1;
                    }
                }
            }
        }
    };
    let elapsed = t0.elapsed().as_secs_f64();
    let lat = sorted(lat);
    println!("\n-- summary");
    println!("ended: {end}");
    println!("first update after: {:?}", first_update);
    println!(
        "transactions: {txs} ({:.1}/s), unique {}, duplicates {dups}",
        txs as f64 / elapsed,
        sigs.len()
    );
    println!(
        "tx slot range: {}..={max_slot}",
        if min_slot == u64::MAX { 0 } else { min_slot }
    );
    println!(
        "server→client latency ms: p50 {:.1}  p90 {:.1}  p99 {:.1}  (clock skew included)",
        pct(&lat, 0.5),
        pct(&lat, 0.9),
        pct(&lat, 0.99)
    );
    let steady = sorted(steady_lat);
    println!(
        "steady-state latency ms (after t+10s): p50 {:.1}  p90 {:.1}  p99 {:.1}",
        pct(&steady, 0.5),
        pct(&steady, 0.9),
        pct(&steady, 0.99)
    );
    println!("slot updates by status: {statuses:?}");
    println!("transactions arriving after their slot's SlotProcessed update: {late}");
    println!(
        "pings: {}  transactions after first ping: {txs_after_ping}",
        s.pings
    );
    Ok(())
}

async fn replay_info(env: &Env) -> Result<Option<u64>> {
    let channel = Channel::from_shared(env.grpc_url.clone())?
        .tls_config(ClientTlsConfig::new().with_native_roots())?
        .connect()
        .await?;
    let mut client = GeyserClient::new(channel);
    let mut req = tonic::Request::new(SubscribeReplayInfoRequest {});
    req.metadata_mut().insert("x-token", env.api_key.parse()?);
    Ok(client
        .subscribe_replay_info(req)
        .await?
        .into_inner()
        .first_available)
}

/// Spike 2: how far back can we replay, how fast does it catch up, and is it ordered?
pub async fn replay(env: &Env, back: u64, secs: u64, zstd: bool) -> Result<()> {
    let rpc = Rpc::new(env);
    let tip = rpc.get_slot("processed").await?;
    match replay_info(env).await {
        Ok(Some(first)) => println!(
            "SubscribeReplayInfo: first_available {first} → horizon {} slots behind tip {tip}",
            tip.saturating_sub(first)
        ),
        Ok(None) => println!("SubscribeReplayInfo: first_available not reported"),
        Err(e) => println!("SubscribeReplayInfo failed: {e}"),
    }
    let from = tip.saturating_sub(back);
    println!(
        "tip (processed) {tip}; subscribing with from_slot {from} ({back} slots back, zstd: {zstd})"
    );

    let t0 = Instant::now();
    let req = request(&env.program, CommitmentLevel::Processed, Some(from));
    let opened = if zstd {
        Session::open_zstd(env, req, true).await
    } else {
        Session::open(env, req, true).await
    };
    let mut s = match opened {
        Ok(s) => s,
        Err(e) => {
            println!("subscribe rejected: {e}");
            return Ok(());
        }
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.tick().await;

    let mut first_tx: Option<(Duration, u64)> = None;
    let mut first_slot_update: Option<(u64, String)> = None;
    let mut caught_up: Option<Duration> = None;
    let (mut replayed, mut live, mut regressions, mut dups) = (0u64, 0u64, 0u64, 0u64);
    let mut max_slot = 0u64;
    let mut sigs = HashSet::new();
    let mut replay_statuses: BTreeMap<String, u64> = BTreeMap::new();
    let end = loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break "time limit reached".to_owned(),
            _ = tick.tick() => {
                println!("  t+{:>3}s  max tx slot {max_slot} ({:+} vs tip at start)  replayed {replayed}  live {live}",
                    t0.elapsed().as_secs(), max_slot as i64 - tip as i64);
                if let Some(at) = caught_up && t0.elapsed() > at + Duration::from_secs(10) {
                    break "caught up; stopped 10s later".to_owned();
                }
            }
            item = s.next() => match item {
                None => break "server closed the stream".to_owned(),
                Some(Err(st)) => break format!("stream error {}", describe(&st)),
                Some(Ok(u)) => {
                    if let Some((slot, sig)) = tx_of(&u) {
                        first_tx.get_or_insert((t0.elapsed(), slot));
                        if slot < max_slot { regressions += 1; }
                        max_slot = max_slot.max(slot);
                        if slot < tip { replayed += 1; } else { live += 1; }
                        if !sigs.insert(sig) { dups += 1; }
                        if caught_up.is_none() && slot >= tip { caught_up = Some(t0.elapsed()); }
                    } else if let Some((slot, status)) = slot_status(&u) {
                        first_slot_update.get_or_insert((slot, format!("{status:?}")));
                        if slot < tip { *replay_statuses.entry(format!("{status:?}")).or_default() += 1; }
                    }
                }
            }
        }
    };
    println!("\n-- summary");
    println!("ended: {end}");
    println!("first tx: {first_tx:?} (from_slot was {from})");
    println!("first slot update: {first_slot_update:?}");
    println!("caught up to tip-at-start after: {caught_up:?}");
    println!("replayed txs (slot < tip): {replayed}; live txs: {live}; duplicates: {dups}");
    println!("slot regressions (tx slot lower than one already seen): {regressions}");
    println!("slot statuses seen for replayed slots: {replay_statuses:?}");
    Ok(())
}

/// Spike 4: kill our own stream through the account API and capture how the client sees it,
/// then resume from the last slot seen and count the overlap a naive consumer would double-count.
pub async fn kill(env: &Env, after: u64) -> Result<()> {
    let api = Api::new(env);
    let auth = api.find_auth().await?;
    println!("account API accepts {auth:?}");
    let before: HashSet<String> = ids(&api.list_grpc(auth).await?);

    let mut s = Session::open(
        env,
        request(&env.program, CommitmentLevel::Processed, None),
        true,
    )
    .await?;
    let seen = Arc::new(Mutex::new((0u64, HashSet::<String>::new())));
    let (done_tx, done_rx) = oneshot::channel::<(Instant, String)>();
    let seen_task = seen.clone();
    tokio::spawn(async move {
        let reason = loop {
            match s.next().await {
                None => break "server closed the stream without a status".to_owned(),
                Some(Err(st)) => break describe(&st),
                Some(Ok(u)) => {
                    if let Some((slot, sig)) = tx_of(&u) {
                        let mut g = seen_task.lock().unwrap();
                        g.0 = g.0.max(slot);
                        g.1.insert(sig);
                    }
                }
            }
        };
        let _ = done_tx.send((Instant::now(), reason));
    });

    tokio::time::sleep(Duration::from_secs(after)).await;
    let conns = api.list_grpc(auth).await?;
    println!("\nlive connections ({}):", conns.len());
    for c in &conns {
        println!(
            "  {}  started {}  region {}  bytes {}  bps {}  buffer {}/{}  paygo {}",
            c["conn_id"],
            c["started_at"],
            c["region"],
            c["bytes_streamed"],
            c["throughput_bps"],
            c["buffer_pending"],
            c["buffer_size"],
            c["is_paygo"]
        );
    }
    let ours = conns
        .iter()
        .filter(|c| c["conn_id"].as_str().is_some_and(|id| !before.contains(id)))
        .max_by_key(|c| c["started_at"].as_u64().unwrap_or(0))
        .and_then(|c| c["conn_id"].as_str().map(str::to_owned));
    let Some(conn_id) = ours else {
        bail!("could not identify our connection in the list")
    };

    let t_kill = Instant::now();
    let (status, body) = api.kill(auth, &conn_id).await?;
    println!(
        "\nDELETE {conn_id} → HTTP {} {}",
        status.as_u16(),
        body.trim()
    );
    match tokio::time::timeout(Duration::from_secs(20), done_rx).await {
        Ok(Ok((at, reason))) => {
            println!(
                "stream ended {} ms after the kill call: {reason}",
                at.duration_since(t_kill).as_millis()
            )
        }
        _ => println!("stream still open 20s after the kill call"),
    }

    let (last_slot, before_sigs) = {
        let g = seen.lock().unwrap();
        (g.0, g.1.clone())
    };
    println!(
        "\nbefore the kill: {} txs, last slot {last_slot}",
        before_sigs.len()
    );
    println!("resuming with from_slot = {last_slot} (the last slot seen, not +1)…");
    let mut r = Session::open(
        env,
        request(&env.program, CommitmentLevel::Processed, Some(last_slot)),
        true,
    )
    .await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let (mut total, mut overlap, mut first_slot) = (0u64, 0u64, None);
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break,
            item = r.next() => match item {
                Some(Ok(u)) => if let Some((slot, sig)) = tx_of(&u) {
                    first_slot.get_or_insert(slot);
                    total += 1;
                    if before_sigs.contains(&sig) { overlap += 1; }
                },
                Some(Err(st)) => { println!("resume stream error {}", describe(&st)); break; }
                None => break,
            }
        }
    }
    println!(
        "resumed: first tx slot {first_slot:?}, {total} txs in 10s, {overlap} already seen before the kill"
    );
    Ok(())
}

fn ids(conns: &[serde_json::Value]) -> HashSet<String> {
    conns
        .iter()
        .filter_map(|c| c["conn_id"].as_str().map(str::to_owned))
        .collect()
}

/// Record the live stream as frames of `[u64 LE receive-time nanos][length-delimited SubscribeUpdate]`.
pub async fn record(env: &Env, secs: u64, out: Option<PathBuf>) -> Result<()> {
    let out = out.unwrap_or_else(|| {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        PathBuf::from(format!("fixtures/raw/capture-{ts}.bin"))
    });
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::io::BufWriter::new(std::fs::File::create(&out)?);
    println!("recording {} for {secs}s → {}", env.program, out.display());

    let mut s = Session::open(
        env,
        request(&env.program, CommitmentLevel::Processed, None),
        true,
    )
    .await?;
    let t0 = Instant::now();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    let mut tick = tokio::time::interval(Duration::from_secs(10));
    tick.tick().await;
    let (mut frames, mut txs, mut bytes) = (0u64, 0u64, 0usize);
    let mut buf = Vec::with_capacity(4096);
    let end = loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break "time limit reached".to_owned(),
            _ = tick.tick() => println!("  t+{:>4}s  frames {frames}  txs {txs}  {:.1} MB", t0.elapsed().as_secs(), bytes as f64 / 1e6),
            item = s.next() => match item {
                None => break "server closed the stream".to_owned(),
                Some(Err(st)) => break format!("stream error {}", describe(&st)),
                Some(Ok(u)) => {
                    if matches!(u.update_oneof, Some(UpdateOneof::Ping(_))) { continue; }
                    if tx_of(&u).is_some() { txs += 1; }
                    buf.clear();
                    buf.extend_from_slice(&(now_nanos() as u64).to_le_bytes());
                    u.encode_length_delimited(&mut buf)?;
                    file.write_all(&buf)?;
                    frames += 1;
                    bytes += buf.len();
                }
            }
        }
    };
    file.flush()?;
    println!(
        "done ({end}): {frames} frames, {txs} txs, {:.1} MB",
        bytes as f64 / 1e6
    );
    Ok(())
}

/// Which unary Geyser RPCs does Solami answer, and what metadata comes back on subscribe?
pub async fn unary(env: &Env) -> Result<()> {
    use solami::geyser::{GetSlotRequest, GetVersionRequest};
    let channel = Channel::from_shared(env.grpc_url.clone())?
        .tls_config(ClientTlsConfig::new().with_native_roots())?
        .connect()
        .await?;
    let mut client = GeyserClient::new(channel);
    for commitment in [
        CommitmentLevel::Processed,
        CommitmentLevel::Confirmed,
        CommitmentLevel::Finalized,
    ] {
        let t = Instant::now();
        let res = client
            .get_slot(authed_stream(
                env,
                GetSlotRequest {
                    commitment: Some(commitment as i32),
                },
            )?)
            .await;
        match res {
            Ok(r) => println!(
                "GetSlot {commitment:?}: {} ({} ms)",
                r.into_inner().slot,
                t.elapsed().as_millis()
            ),
            Err(st) => println!("GetSlot {commitment:?}: {}", describe(&st)),
        }
    }
    match client
        .get_version(authed_stream(env, GetVersionRequest {})?)
        .await
    {
        Ok(r) => println!("GetVersion: {}", r.into_inner().version),
        Err(st) => println!("GetVersion: {}", describe(&st)),
    }
    let (sink, rx) = mpsc::channel(4);
    sink.send(request(&env.program, CommitmentLevel::Processed, None))
        .await?;
    let resp = client
        .subscribe(authed_stream(env, ReceiverStream::new(rx))?)
        .await?;
    println!("subscribe response metadata:");
    for kv in resp.metadata().iter() {
        match kv {
            tonic::metadata::KeyAndValueRef::Ascii(k, v) => {
                println!("  {k}: {}", v.to_str().unwrap_or("?"))
            }
            tonic::metadata::KeyAndValueRef::Binary(k, _) => println!("  {k}: <binary>"),
        }
    }
    Ok(())
}

fn authed_stream<T>(env: &Env, body: T) -> Result<tonic::Request<T>> {
    let mut req = tonic::Request::new(body);
    req.metadata_mut().insert("x-token", env.api_key.parse()?);
    Ok(req)
}
