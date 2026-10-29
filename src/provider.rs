use crate::{Application, chat::Part};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use serde_json::{Value, json};
use sqlx::Row;
use tokio::sync::mpsc;
use uuid::Uuid;

pub(crate) enum Piece {
    Delta(String),
    Usage(Value),
    Receipt(String),
    Image(crate::attachments::ValidatedImage),
}
pub(crate) struct Context {
    messages: Vec<Value>,
    prompt: String,
    current_images: Vec<String>,
}
// Actual replaceable IO boundary is HTTP; protocol simulators exercise these same adapters.
pub(crate) async fn context(
    app: &Application,
    owner: Uuid,
    chat: Uuid,
    user: Uuid,
    image_output: bool,
) -> anyhow::Result<Context> {
    let rows=sqlx::query("WITH RECURSIVE path AS (SELECT id,parent_id,role,parts,depth FROM messages WHERE id=$1 AND chat_id=$2 UNION ALL SELECT m.id,m.parent_id,m.role,m.parts,m.depth FROM messages m JOIN path p ON m.id=p.parent_id WHERE m.chat_id=$2) SELECT p.* FROM path p JOIN chats c ON c.id=$2 WHERE c.owner_id=$3 AND c.deleted_at IS NULL ORDER BY p.depth")
        .bind(user).bind(chat).bind(owner).fetch_all(&app.0.pool).await?;
    anyhow::ensure!(!rows.is_empty() && rows.len() <= 128, "context unavailable");
    let mut messages = Vec::new();
    let mut text_bytes = 0;
    let mut image_bytes = 0;
    let mut prompt = String::new();
    let mut current_images = Vec::new();
    for row in rows {
        let role: String = row.get("role");
        let id: Uuid = row.get("id");
        let parts: Vec<Part> = serde_json::from_value(row.get("parts"))?;
        let mut content = Vec::new();
        let mut assistant_text = String::new();
        for part in parts {
            match part {
                Part::Text { text } => {
                    text_bytes += text.len();
                    if id == user {
                        prompt.push_str(&text);
                        prompt.push('\n');
                    }
                    if role == "assistant" {
                        assistant_text.push_str(&text);
                    } else {
                        content.push(json!({"type":"text","text":text}));
                    }
                }
                Part::Image { attachment_id } => {
                    if image_output && id != user {
                        continue;
                    }
                    if role == "assistant" {
                        assistant_text.push_str("[Assistant generated an image; image pixels are not included in this text context.]");
                        prompt.push_str("assistant: [Previously generated image; pixels not available to this text-only image generation adapter.]\n");
                    } else {
                        let a=sqlx::query("SELECT a.mime,a.bytes FROM attachments a JOIN message_attachments ma ON ma.attachment_id=a.id WHERE a.id=$1 AND a.owner_id=$2 AND ma.message_id=$3")
                            .bind(attachment_id).bind(owner).bind(id).fetch_one(&app.0.pool).await?;
                        let bytes: Vec<u8> = a.get("bytes");
                        image_bytes += bytes.len();
                        anyhow::ensure!(image_bytes <= 32 * 1024 * 1024, "context limit");
                        let encoded = STANDARD.encode(bytes);
                        let image = json!({"type":"image_url","image_url":{"url":format!("data:{};base64,{}",a.get::<String,_>("mime"),encoded)}});
                        if id == user {
                            current_images.push(encoded);
                        }
                        content.push(image);
                    }
                }
            }
        }
        anyhow::ensure!(text_bytes <= 65536, "context limit");
        messages.push(if role == "assistant" {
            json!({"role":role,"content":assistant_text})
        } else {
            json!({"role":role,"content":content})
        });
    }
    Ok(Context {
        messages,
        prompt,
        current_images,
    })
}
pub(crate) async fn generate(
    app: Application,
    output: String,
    context: Context,
    tx: mpsc::Sender<Piece>,
) -> anyhow::Result<()> {
    if output == "text" {
        deepseek(&app, context, tx).await
    } else {
        bfl(&app, context, tx).await
    }
}
async fn bounded_json(response: reqwest::Response) -> anyhow::Result<Value> {
    anyhow::ensure!(response.status().is_success(), "provider rejected request");
    let bytes = bounded_bytes(response, 65536).await?;
    Ok(serde_json::from_slice(&bytes)?)
}
async fn bounded_bytes(response: reqwest::Response, max: usize) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(response.status().is_success(), "download rejected");
    if let Some(n) = response.content_length() {
        anyhow::ensure!(n <= max as u64, "response limit");
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        anyhow::ensure!(bytes.len() + chunk.len() <= max, "response limit");
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
async fn deepseek(
    app: &Application,
    context: Context,
    tx: mpsc::Sender<Piece>,
) -> anyhow::Result<()> {
    let base = app
        .0
        .config
        .providers
        .local_simulator
        .as_deref()
        .unwrap_or("https://api.deepseek.com");
    let response=app.0.client.post(format!("{base}/chat/completions"))
        .bearer_auth(app.0.config.providers.deepseek_key.as_deref().ok_or_else(||anyhow::anyhow!("unavailable"))?)
        .json(&json!({"model":"deepseek-flash","messages":context.messages,"stream":true,"stream_options":{"include_usage":true},"thinking":{"type":"disabled"},"max_tokens":8192}))
        .send().await?;
    anyhow::ensure!(response.status().is_success(), "provider rejected request");
    let mut stream = response.bytes_stream();
    let mut parser = Frames::default();
    let mut finish = false;
    let mut done = false;
    let mut total = 0usize;
    while let Some(chunk) = stream.next().await {
        for data in parser.push(&chunk?)? {
            if data == "[DONE]" {
                anyhow::ensure!(finish, "missing finish");
                done = true;
                break;
            }
            let value: Value = serde_json::from_str(&data)?;
            anyhow::ensure!(value.get("error").is_none(), "provider stream error");
            if let Some(usage) = value.get("usage").filter(|u| !u.is_null()) {
                tx.send(Piece::Usage(json!({
                    "input_tokens": usage.get("prompt_tokens").and_then(Value::as_u64),
                    "output_tokens": usage.get("completion_tokens").and_then(Value::as_u64),
                    "total_tokens": usage.get("total_tokens").and_then(Value::as_u64),
                })))
                .await?;
            }
            if let Some(choices) = value.get("choices").and_then(Value::as_array) {
                anyhow::ensure!(choices.len() <= 1, "unexpected choices");
                for choice in choices {
                    if let Some(text) = choice.pointer("/delta/content").and_then(Value::as_str) {
                        anyhow::ensure!(!finish, "content after finish");
                        total += text.len();
                        anyhow::ensure!(total <= 65536, "output limit");
                        if !text.is_empty() {
                            tx.send(Piece::Delta(text.into())).await?;
                        }
                    }
                    anyhow::ensure!(
                        choice.pointer("/delta/tool_calls").is_none(),
                        "unsupported tools"
                    );
                    if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                        anyhow::ensure!(reason == "stop", "truncated or unsupported finish");
                        finish = true;
                    }
                }
            }
            // reasoning_content is deliberately never forwarded or stored.
        }
        if done {
            break;
        }
    }
    anyhow::ensure!(done && finish && total > 0, "incomplete provider stream");
    Ok(())
}
#[derive(Default)]
struct Frames {
    bytes: Vec<u8>,
    data: String,
    frame_size: usize,
}
impl Frames {
    // SSE framing is byte-oriented until complete lines, accepting CRLF and multi-line data.
    fn push(&mut self, chunk: &[u8]) -> anyhow::Result<Vec<String>> {
        let mut frames = Vec::new();
        for &byte in chunk {
            self.frame_size += 1;
            anyhow::ensure!(self.frame_size <= 65536, "SSE frame limit");
            if byte != b'\n' {
                self.bytes.push(byte);
                continue;
            }
            if self.bytes.last() == Some(&b'\r') {
                self.bytes.pop();
            }
            let line = std::str::from_utf8(&self.bytes)?;
            if line.is_empty() {
                if !self.data.is_empty() {
                    self.data.pop();
                    frames.push(std::mem::take(&mut self.data));
                }
                self.frame_size = 0;
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data.push_str(data.strip_prefix(' ').unwrap_or(data));
                self.data.push('\n');
            }
            self.bytes.clear();
        }
        Ok(frames)
    }
}
fn safe_url(app: &Application, s: &str, delivery: bool) -> anyhow::Result<reqwest::Url> {
    let url = reqwest::Url::parse(s)?;
    anyhow::ensure!(
        s.len() <= 4096
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "unsafe provider URL"
    );
    if let Some(base) = &app.0.config.providers.local_simulator {
        let base = reqwest::Url::parse(base)?;
        anyhow::ensure!(
            url.origin() == base.origin(),
            "simulator URL must stay on configured origin"
        );
        return Ok(url);
    }
    anyhow::ensure!(
        url.scheme() == "https" && url.port_or_known_default() == Some(443),
        "unsafe provider URL"
    );
    let host = url.host_str().unwrap_or_default();
    let labels: Vec<&str> = host.split('.').collect();
    let dns_label = |s: &str| {
        !s.is_empty()
            && s.len() <= 63
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && s.as_bytes()[0].is_ascii_alphanumeric()
            && s.as_bytes()[s.len() - 1].is_ascii_alphanumeric()
    };
    let allowed = if delivery {
        labels.len() == 4
            && labels[0] == "delivery"
            && dns_label(labels[1])
            && labels[2..] == ["bfl", "ai"]
    } else {
        host == "api.bfl.ai"
            || (labels.len() == 4
                && labels[0] == "api"
                && dns_label(labels[1])
                && labels[2..] == ["bfl", "ai"])
    };
    anyhow::ensure!(allowed, "unapproved provider URL host");
    Ok(url)
}
async fn bfl(app: &Application, context: Context, tx: mpsc::Sender<Piece>) -> anyhow::Result<()> {
    anyhow::ensure!(!context.prompt.trim().is_empty(), "image prompt required");
    let base = app
        .0
        .config
        .providers
        .local_simulator
        .as_deref()
        .unwrap_or("https://api.bfl.ai");
    let key = app
        .0
        .config
        .providers
        .bfl_key
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("unavailable"))?;
    let response = bounded_json(
        app.0
            .client
            .post(format!("{base}/v1/flux-2-klein-4b"))
            .header("x-key", key)
            .json(&{
                let mut payload = json!({"prompt":context.prompt,"width":1024,"height":1024});
                if let Some(map) = payload.as_object_mut() {
                    for (index, image) in context.current_images.iter().enumerate() {
                        map.insert(
                            if index == 0 {
                                "input_image".into()
                            } else {
                                format!("input_image_{}", index + 1)
                            },
                            Value::String(image.clone()),
                        );
                    }
                }
                payload
            })
            .send()
            .await?,
    )
    .await?;
    let receipt = response
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| s.len() <= 256)
        .ok_or_else(|| anyhow::anyhow!("missing receipt"))?;
    tx.send(Piece::Receipt(receipt.into())).await?;
    let polling = safe_url(
        app,
        response
            .get("polling_url")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing polling URL"))?,
        false,
    )?;
    loop {
        let status = bounded_json(
            app.0
                .client
                .get(polling.clone())
                .header("x-key", key)
                .send()
                .await?,
        )
        .await?;
        match status.get("status").and_then(Value::as_str) {
            Some("Ready") => {
                let sample = status
                    .pointer("/result/sample")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("missing image"))?;
                let url = safe_url(app, sample, true)?;
                // Deliberately no provider credentials on the delivery request, and no redirects.
                let bytes =
                    bounded_bytes(app.0.client.get(url).send().await?, 8 * 1024 * 1024).await?;
                let image = crate::attachments::validate(app, bytes, None)
                    .await
                    .map_err(|_| anyhow::anyhow!("invalid output image"))?;
                tx.send(Piece::Image(image)).await?;
                return Ok(());
            }
            Some("Pending" | "Processing") => {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await
            }
            _ => anyhow::bail!("image generation failed"),
        }
    }
}
