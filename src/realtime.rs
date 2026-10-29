//! One-way, owner-scoped cache invalidations; generation deltas use their existing stream.
use crate::{
    Application,
    auth::Auth,
    http::{Error, Result},
};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::{collections::BTreeMap, time::Duration};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    after: Option<i64>,
}

struct Change {
    seq: i64,
    resource: String,
    id: Uuid,
    action: String,
}
struct Page {
    alive: bool,
    high: i64,
    low: Option<i64>,
    changes: Vec<Change>,
}

async fn page(
    app: &Application,
    auth: &Auth,
    after: i64,
) -> std::result::Result<Page, sqlx::Error> {
    // Authentication, retention boundary and replay rows share one MVCC snapshot.
    // Every connection is returned before yielding or waiting for another batch.
    let rows=sqlx::query("WITH bounds AS (SELECT EXISTS(SELECT 1 FROM sessions WHERE user_id=$1 AND token_hash=$2 AND expires_at>now()) AS alive,COALESCE((SELECT last_seq FROM realtime_clocks WHERE owner_id=$1),0)::bigint AS high,(SELECT min(seq) FROM realtime_events WHERE owner_id=$1 AND created_at>=now()-interval '7 days') AS low), page AS (SELECT seq,resource,resource_id,action FROM realtime_events WHERE owner_id=$1 AND seq>$3 AND created_at>=now()-interval '7 days' ORDER BY seq LIMIT 128) SELECT b.alive,b.high,b.low,p.seq,p.resource,p.resource_id,p.action FROM bounds b LEFT JOIN page p ON b.alive ORDER BY p.seq")
        .bind(auth.user).bind(&auth.hash).bind(after).fetch_all(&app.0.pool).await?;
    let first = &rows[0];
    let mut result = Page {
        alive: first.get("alive"),
        high: first.get("high"),
        low: first.get("low"),
        changes: Vec::new(),
    };
    for row in rows {
        if let Some(seq) = row.get::<Option<i64>, _>("seq") {
            result.changes.push(Change {
                seq,
                resource: row.get("resource"),
                id: row.get("resource_id"),
                action: row.get("action"),
            });
        }
    }
    Ok(result)
}

pub(crate) async fn events(
    State(app): State<Application>,
    auth: Auth,
    Query(query): Query<Cursor>,
    headers: HeaderMap,
) -> Result<Response> {
    let header = headers
        .get("last-event-id")
        .map(|v| {
            v.to_str()
                .ok()
                .filter(|s| s.len() <= 20)
                .and_then(|s| s.parse::<i64>().ok())
                .ok_or_else(Error::bad)
        })
        .transpose()?;
    if query.after.is_some() && header.is_some() && query.after != header {
        return Err(Error::bad());
    }
    let cursor = query.after.or(header);
    let mut after = cursor.unwrap_or(0);
    if after < 0 {
        return Err(Error::bad());
    }
    let initial = page(&app, &auth, after).await?;
    if !initial.alive {
        return Err(Error(StatusCode::UNAUTHORIZED, "unauthorized"));
    }
    if after > initial.high {
        return Err(Error::bad());
    }
    let permit = app
        .0
        .streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "stream_capacity"))?;
    let stream = async_stream::stream! {
        let _permit=permit;
        let mut pending=Some(initial);
        let mut first=true;
        loop {
            let current=if let Some(page)=pending.take(){page}else{match page(&app,&auth,after).await{Ok(page)=>page,Err(_)=>{yield Err::<Event,std::io::Error>(std::io::Error::other("stream unavailable"));break;}}};
            if !current.alive {break;}
            let expired=match current.low {Some(low)=>after<low-1,None=>after<current.high};
            let count=current.changes.len();
            if (first && cursor.is_none()) || expired {
                after=current.high;
                yield Ok(Event::default().id(after.to_string()).event("resync").data(json!({"seq":after,"action":"resync"}).to_string()));
            } else if !current.changes.is_empty() {
                let mut latest=BTreeMap::new();
                for change in current.changes {
                    after=change.seq;
                    latest.insert((change.resource.clone(),change.id),change);
                }
                let mut changes=latest.into_values().collect::<Vec<_>>();
                changes.sort_by_key(|change|change.seq);
                let changes:Vec<Value>=changes.into_iter().map(|change|json!({"resource":change.resource,"id":change.id,"action":change.action})).collect();
                yield Ok(Event::default().id(after.to_string()).event("changes").data(json!({"seq":after,"changes":changes}).to_string()));
            }
            first=false;
            if count<128 || expired || cursor.is_none() {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    };
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response())
}
