use crate::{
    Application, attachments, chat,
    provider::{self, Piece},
};
use serde_json::{Value, json};
use sqlx::Row;
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
    time::{Duration, Instant},
};
use uuid::Uuid;

struct Job {
    id: Uuid,
    owner: Uuid,
    chat: Uuid,
    user: Uuid,
    lease: Uuid,
    output: String,
}
pub(crate) async fn run(app: Application, shutdown: watch::Receiver<bool>) -> anyhow::Result<()> {
    let mut workers = JoinSet::new();
    for _ in 0..app.0.config.workers {
        workers.spawn(worker(app.clone(), shutdown.clone()));
    }
    while let Some(result) = workers.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                workers.abort_all();
                while workers.join_next().await.is_some() {}
                return Err(e);
            }
            Err(_) => {
                workers.abort_all();
                return Err(anyhow::anyhow!("worker task failed"));
            }
        }
    }
    Ok(())
}
async fn worker(app: Application, mut shutdown: watch::Receiver<bool>) -> anyhow::Result<()> {
    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        reap(&app).await?;
        if let Some(job) = claim(&app).await? {
            execute(&app, job, &mut shutdown).await?;
        } else {
            tokio::select! {_ = tokio::time::sleep(Duration::from_millis(200))=>{},_ = shutdown.changed()=>{return Ok(());}}
        }
    }
}
async fn claim(app: &Application) -> anyhow::Result<Option<Job>> {
    let mut tx = app.0.pool.begin().await?;
    let row=sqlx::query("SELECT g.* FROM generations g WHERE g.status='queued' AND EXISTS(SELECT 1 FROM chats c WHERE c.id=g.chat_id AND c.owner_id=g.owner_id AND c.deleted_at IS NULL) ORDER BY g.created_at,g.id FOR UPDATE OF g SKIP LOCKED LIMIT 1").fetch_optional(&mut *tx).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let id = row.get("id");
    let lease = Uuid::new_v4();
    sqlx::query("UPDATE generations SET status='running',lease=$2,lease_until=now()+interval '10 seconds',updated_at=now() WHERE id=$1").bind(id).bind(lease).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Some(Job {
        id,
        lease,
        owner: row.get("owner_id"),
        chat: row.get("chat_id"),
        user: row.get("user_message_id"),
        output: row.get("output"),
    }))
}
async fn reap(app: &Application) -> anyhow::Result<()> {
    let rows=sqlx::query("SELECT id,chat_id,owner_id FROM generations WHERE status='running' AND lease_until<now() ORDER BY lease_until LIMIT 16").fetch_all(&app.0.pool).await?;
    for row in rows {
        let mut tx = app.0.pool.begin().await?;
        let id: Uuid = row.get("id");
        sqlx::query("SELECT id FROM chats WHERE id=$1 AND owner_id=$2 FOR UPDATE")
            .bind(row.get::<Uuid, _>("chat_id"))
            .bind(row.get::<Uuid, _>("owner_id"))
            .execute(&mut *tx)
            .await?;
        let expired:Option<Uuid>=sqlx::query_scalar("SELECT id FROM generations WHERE id=$1 AND status='running' AND lease_until<now() FOR UPDATE").bind(id).fetch_optional(&mut *tx).await?;
        if expired.is_some() {
            chat::terminal(
                &mut tx,
                id,
                "failed",
                Some("interrupted_unknown_provider_outcome"),
            )
            .await?;
        }
        tx.commit().await?;
    }
    Ok(())
}
struct Accumulator {
    text: String,
    pending: String,
    usage: Option<Value>,
    image: Option<attachments::ValidatedImage>,
    events: usize,
}
impl Accumulator {
    fn piece(&mut self, piece: Piece) -> anyhow::Result<Option<String>> {
        match piece {
            Piece::Delta(text) => {
                anyhow::ensure!(self.text.len() + text.len() <= 65536, "output limit");
                self.text.push_str(&text);
                self.pending.push_str(&text);
            }
            Piece::Usage(usage) => self.usage = Some(usage),
            Piece::Receipt(receipt) => return Ok(Some(receipt)),
            Piece::Image(image) => self.image = Some(image),
        }
        Ok(None)
    }
}
async fn execute(
    app: &Application,
    job: Job,
    shutdown: &mut watch::Receiver<bool>,
) -> anyhow::Result<()> {
    // Context assembly/decode/provider IO run under the same cancellation/lease loop.
    let (tx, mut rx) = mpsc::channel(8);
    let provider_app = app.clone();
    let owner = job.owner;
    let chat = job.chat;
    let user = job.user;
    let output = job.output.clone();
    let future = async move {
        let context =
            provider::context(&provider_app, owner, chat, user, output == "image").await?;
        provider::generate(provider_app, output, context, tx).await
    };
    tokio::pin!(future);
    let deadline = tokio::time::sleep(Duration::from_secs(120));
    tokio::pin!(deadline);
    let mut ticks = tokio::time::interval(Duration::from_millis(250));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_flush = Instant::now();
    let mut acc = Accumulator {
        text: String::new(),
        pending: String::new(),
        usage: None,
        image: None,
        events: 0,
    };
    let outcome = loop {
        tokio::select! {
            _=shutdown.changed()=>{break Err("interrupted_unknown_provider_outcome");}
            _=&mut deadline=>{break Err("generation_timeout_unknown_provider_outcome");}
            result=&mut future=>{
                while let Ok(piece)=rx.try_recv(){
                    if let Some(receipt)=acc.piece(piece)? && !receipt_save(app,&job,&receipt).await? {return Ok(());}
                }
                break result.map_err(|_|"provider_failed");
            }
            piece=rx.recv()=>{
                if let Some(piece)=piece && let Some(receipt)=acc.piece(piece)? && !receipt_save(app,&job,&receipt).await? {return Ok(());}
                if acc.pending.len()>=1024 {
                    if !flush(app,&job,&mut acc).await?{return Ok(());}last_flush=Instant::now();
                }
            }
            _=ticks.tick()=>{
                let renewed=sqlx::query("UPDATE generations g SET lease_until=now()+interval '10 seconds' WHERE g.id=$1 AND g.owner_id=$2 AND g.lease=$3 AND g.status='running' AND g.lease_until>now() AND EXISTS(SELECT 1 FROM chats c WHERE c.id=g.chat_id AND c.owner_id=$2 AND c.deleted_at IS NULL)")
                    .bind(job.id).bind(job.owner).bind(job.lease).execute(&app.0.pool).await?.rows_affected();
                if renewed==0 {return Ok(());}
                if !acc.pending.is_empty() && last_flush.elapsed()>=Duration::from_millis(250) {
                    if !flush(app,&job,&mut acc).await?{return Ok(());}last_flush=Instant::now();
                }
            }
        }
    };
    // Dropping the pinned provider future on exit stops local IO; remote charges cannot be undone.
    if !flush(app, &job, &mut acc).await? {
        return Ok(());
    }
    finish(app, &job, acc, outcome.err()).await
}
async fn receipt_save(app: &Application, job: &Job, receipt: &str) -> anyhow::Result<bool> {
    Ok(sqlx::query("UPDATE generations SET provider_receipt=$4 WHERE id=$1 AND owner_id=$2 AND lease=$3 AND status='running' AND lease_until>now()")
        .bind(job.id).bind(job.owner).bind(job.lease).bind(receipt).execute(&app.0.pool).await?.rows_affected()==1)
}
async fn fence(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, job: &Job) -> anyhow::Result<bool> {
    let live: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM chats WHERE id=$1 AND owner_id=$2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(job.chat)
    .bind(job.owner)
    .fetch_optional(&mut **tx)
    .await?;
    if live.is_none() {
        return Ok(false);
    }
    let fenced:Option<Uuid>=sqlx::query_scalar("SELECT id FROM generations WHERE id=$1 AND owner_id=$2 AND lease=$3 AND status='running' AND lease_until>now() FOR UPDATE").bind(job.id).bind(job.owner).bind(job.lease).fetch_optional(&mut **tx).await?;
    Ok(fenced.is_some())
}
async fn flush(app: &Application, job: &Job, acc: &mut Accumulator) -> anyhow::Result<bool> {
    if acc.pending.is_empty() {
        return Ok(true);
    }
    anyhow::ensure!(acc.events < 1020, "event limit");
    let mut tx = app.0.pool.begin().await?;
    if !fence(&mut tx, job).await? {
        return Ok(false);
    }
    chat::event(&mut tx, job.id, "delta", json!({"text":acc.pending})).await?;
    tx.commit().await?;
    acc.pending.clear();
    acc.events += 1;
    Ok(true)
}
async fn finish(
    app: &Application,
    job: &Job,
    mut acc: Accumulator,
    error: Option<&str>,
) -> anyhow::Result<()> {
    let mut tx = app.0.pool.begin().await?;
    // Same lock order as admission and image uploads, before chat/generation fences.
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(job.owner)
        .execute(&mut *tx)
        .await?;
    if !fence(&mut tx, job).await? {
        return Ok(());
    }
    sqlx::query("UPDATE generations SET usage=$2 WHERE id=$1")
        .bind(job.id)
        .bind(&acc.usage)
        .execute(&mut *tx)
        .await?;
    if let Some(error) = error {
        chat::terminal(&mut tx, job.id, "failed", Some(error)).await?;
        tx.commit().await?;
        return Ok(());
    }
    let mut parts = Vec::new();
    let mut attachment = None;
    if job.output == "image" {
        let Some(image) = acc.image.take() else {
            chat::terminal(&mut tx, job.id, "failed", Some("missing_image")).await?;
            tx.commit().await?;
            return Ok(());
        };
        let available: bool =
            sqlx::query_scalar("SELECT image_bytes+$2<=67108864 FROM users WHERE id=$1")
                .bind(job.owner)
                .bind(image.bytes.len() as i64)
                .fetch_one(&mut *tx)
                .await?;
        if !available {
            chat::terminal(&mut tx, job.id, "failed", Some("image_quota_exceeded")).await?;
            tx.commit().await?;
            return Ok(());
        }
        let meta = attachments::store(&mut tx, job.owner, image, true)
            .await
            .map_err(|_| anyhow::anyhow!("image persistence failed"))?;
        let id: Uuid = serde_json::from_value(meta["id"].clone())?;
        parts.push(json!({"type":"image","attachment_id":id}));
        attachment = Some(id);
        chat::event(&mut tx, job.id, "image", meta).await?;
    } else {
        parts.push(json!({"type":"text","text":acc.text}));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO messages(id,chat_id,parent_id,role,parts,depth,generation_id,ancestors) SELECT $1,$2,m.id,'assistant',$3,m.depth+1,$4,m.ancestors || m.id FROM messages m WHERE m.id=$5 AND m.chat_id=$2")
        .bind(id).bind(job.chat).bind(json!(parts)).bind(job.id).bind(job.user).execute(&mut *tx).await?;
    if let Some(a) = attachment {
        sqlx::query("INSERT INTO message_attachments(message_id,attachment_id) VALUES($1,$2)")
            .bind(id)
            .bind(a)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE chats SET head_id=$3,version=version+1,updated_at=now() WHERE id=$1 AND owner_id=$2 AND head_id=$4 AND deleted_at IS NULL").bind(job.chat).bind(job.owner).bind(id).bind(job.user).execute(&mut *tx).await?;
    sqlx::query("UPDATE generations SET status='completed',result_message_id=$2,lease=NULL,lease_until=NULL,updated_at=now() WHERE id=$1").bind(job.id).bind(id).execute(&mut *tx).await?;
    chat::event(
        &mut tx,
        job.id,
        "completed",
        json!({"status":"completed","result_message_id":id,"usage":acc.usage}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
