//! Real AI texture enhancement — the "вклади API-ключ і воно працює" layer.
//!
//! Provider: **Replicate** (api.replicate.com). One API token unlocks both kinds of model
//! this pipeline needs:
//! - **Upscalers** (default: `nightmareai/real-esrgan`) — resolution enhancement that
//!   faithfully keeps the original content. No prompt involved. The right default for game
//!   textures: layout/colors must survive exactly, or the texture no longer fits its UVs.
//! - **Img2img refiners** (any SDXL-style model on Replicate) — re-detail the texture guided
//!   by a text prompt. `texture_prompt()` builds a per-category prompt from the texture's own
//!   library name. Higher risk/higher reward — opt-in via the `aiModel` setting, never the
//!   default.
//!
//! Configuration lives in the app's own `settings.json` (same file the UI's Settings screen
//! writes): `aiApiKey` (the Replicate token) + optional `aiModel`. The `RISENLAB_AI_KEY` env
//! var overrides the key for one-off CLI runs. No key configured → `load_config()` returns
//! `None` and callers silently keep today's local Lanczos behavior (see `batch::regenerate`),
//! so the whole feature is dormant until the owner pastes a token.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Default Replicate model: faithful upscaling, no prompt, safe for any texture kind.
pub const DEFAULT_MODEL: &str = "nightmareai/real-esrgan";

/// How long to wait for one prediction before giving up (Replicate cold starts can take a
/// minute; real upscales of 1–2k textures run well under that once warm).
const PREDICTION_TIMEOUT: Duration = Duration::from_secs(240);

#[derive(Debug, Clone, PartialEq)]
pub struct AiConfig {
    pub api_key: String,
    /// `owner/name` on Replicate, e.g. "nightmareai/real-esrgan".
    pub model: String,
}

/// The app's settings file — the same one `vite-dev-api.ts` (dev) and the Tauri backend
/// (packaged) read/write: `<home>\Desktop\RisenLab-Project\settings.json`.
pub fn settings_json_path() -> PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
    Path::new(&home).join("Desktop").join("RisenLab-Project").join("settings.json")
}

/// Extracts `(api_key, model)` from the settings JSON text. Separated from file I/O so the
/// parsing is unit-testable. An empty/whitespace key counts as "not configured".
pub fn parse_settings_ai(json: &str) -> Option<AiConfig> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let key = value.get("aiApiKey")?.as_str()?.trim().to_string();
    if key.is_empty() {
        return None;
    }
    let model = value
        .get("aiModel")
        .and_then(|m| m.as_str())
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(DEFAULT_MODEL)
        .to_string();
    Some(AiConfig { api_key: key, model })
}

/// Reads AI config: `RISENLAB_AI_KEY` env var wins (model from settings or default), then the
/// settings file. `None` = feature not configured, callers fall back to local processing.
pub fn load_config() -> Option<AiConfig> {
    let from_settings =
        std::fs::read_to_string(settings_json_path()).ok().and_then(|json| parse_settings_ai(&json));
    if let Ok(env_key) = std::env::var("RISENLAB_AI_KEY") {
        let env_key = env_key.trim().to_string();
        if !env_key.is_empty() {
            let model = from_settings.map(|c| c.model).unwrap_or_else(|| DEFAULT_MODEL.to_string());
            return Some(AiConfig { api_key: env_key, model });
        }
    }
    from_settings
}

/// Builds the per-texture text prompt for img2img-style models, derived from the texture's
/// own library name/path (e.g. "compiled/images/Animation/Monster/Ani_Monster_Wolf_Body_01_
/// Diffuse_S1.png"). Category keywords come from the real naming conventions in the game's
/// own library. Upscaler models ignore prompts entirely — harmless to compute either way.
pub fn texture_prompt(png_rel: &str) -> String {
    let lower = png_rel.to_lowercase();
    let subject = if lower.contains("monster") || lower.contains("wolf") || lower.contains("ogre") {
        "creature skin, fur and hide of a fantasy monster"
    } else if lower.contains("head") || lower.contains("face") || lower.contains("hero") || lower.contains("npc") {
        "realistic human skin, face and hair"
    } else if lower.contains("armor") || lower.contains("wpn") || lower.contains("weapon") || lower.contains("sword") || lower.contains("axe") {
        "weathered metal, steel and leather of medieval weapons and armor"
    } else if lower.contains("nat_") || lower.contains("stone") || lower.contains("rock") || lower.contains("tree") || lower.contains("plant") {
        "natural rock, stone, bark and foliage surfaces"
    } else if lower.contains("arch") || lower.contains("wall") || lower.contains("house") || lower.contains("building") {
        "medieval architecture: plaster, brick, carved stone and timber"
    } else if lower.contains("cloth") || lower.contains("cape") || lower.contains("robe") {
        "woven cloth, linen and rough medieval fabric"
    } else {
        "game asset surface material"
    };
    format!(
        "High-resolution remaster of a video game texture: {subject}. \
         Sharpen and enrich fine surface detail, keep the EXACT same colors, layout, \
         silhouette and UV boundaries as the original image, seamless where the original is \
         seamless, no new objects, no text, photorealistic material detail, 4k quality."
    )
}

/// Whether a texture is a data map (normal/specular) rather than a photo-like image. AI image
/// models are trained on photos and will destroy the encoded vectors — these must always take
/// the local (Lanczos) path regardless of configuration.
pub fn is_data_map(png_rel: &str) -> bool {
    let lower = png_rel.to_lowercase();
    lower.contains("_normal") || lower.contains("_specular") || lower.contains("_nm_")
}

/// Builds the Replicate `input` object for the configured model. Upscalers (the default
/// real-esrgan) take `image` + `scale`; anything else is treated as an img2img refiner and
/// additionally gets the category prompt with a conservative denoising strength (the texture
/// must stay recognizably itself).
pub fn build_input(model: &str, image_data_uri: &str, png_rel: &str, scale: u32) -> serde_json::Value {
    if model.to_lowercase().contains("esrgan") {
        serde_json::json!({
            "image": image_data_uri,
            "scale": scale.clamp(2, 4),
            "face_enhance": false,
        })
    } else {
        serde_json::json!({
            "image": image_data_uri,
            "prompt": texture_prompt(png_rel),
            "negative_prompt": "different colors, changed layout, new objects, text, watermark, frame, blurry",
            "strength": 0.3,
            "guidance_scale": 6.0,
        })
    }
}

/// HTTP via Windows' own bundled `curl.exe` (present since Windows 10 1803) — deliberately
/// NOT a Rust HTTP client: every TLS option (rustls' `ring`, native-tls' windows-sys build)
/// needs a C toolchain/binutils absent in the dev sandbox, while shelling out is the same
/// pattern this codebase already uses for `mimicry-helper.exe`. The request body goes through
/// a temp file (`--data @file`): a base64 texture is megabytes — far past the Windows
/// command-line length limit.
fn curl(args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("curl.exe")
        .args(["-sS", "--max-time", "180"])
        .args(args)
        .output()
        .context("running curl.exe (bundled with Windows 10+)")?;
    if !out.status.success() {
        bail!("curl failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

fn curl_json(args: &[&str]) -> Result<serde_json::Value> {
    let bytes = curl(args)?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("decoding Replicate response: {}", String::from_utf8_lossy(&bytes[..bytes.len().min(400)])))
}

/// Extracts the output image URL from a finished prediction: real-esrgan returns a plain
/// string; img2img models usually return an array of URLs (take the first).
fn output_url(prediction: &serde_json::Value) -> Option<String> {
    match prediction.get("output") {
        Some(serde_json::Value::String(url)) => Some(url.clone()),
        Some(serde_json::Value::Array(items)) => items.first().and_then(|v| v.as_str()).map(String::from),
        _ => None,
    }
}

/// Runs one real enhancement through Replicate and returns the enhanced image bytes
/// (png/jpeg/webp — caller decodes). Blocking by design: this is a CLI.
pub fn enhance_png(cfg: &AiConfig, src_png: &Path, png_rel: &str, scale: u32) -> Result<Vec<u8>> {
    let bytes = std::fs::read(src_png).with_context(|| format!("reading {}", src_png.display()))?;
    let data_uri = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes));
    let auth = format!("Authorization: Bearer {}", cfg.api_key);

    // Request body → temp file (megabytes of base64 blow past the command-line length limit).
    let body = serde_json::json!({ "input": build_input(&cfg.model, &data_uri, png_rel, scale) });
    let body_path = std::env::temp_dir().join(format!("risenlab_ai_{}.json", std::process::id()));
    std::fs::write(&body_path, serde_json::to_vec(&body)?).context("writing request body temp file")?;
    let body_arg = format!("@{}", body_path.display());

    // `Prefer: wait` holds the connection until the prediction finishes (up to ~60s on
    // Replicate's side) — most textures come back in this single round trip. Cold starts
    // fall through to polling below.
    let create_url = format!("https://api.replicate.com/v1/models/{}/predictions", cfg.model);
    let created = curl_json(&[
        "-X", "POST",
        "-H", &auth,
        "-H", "Content-Type: application/json",
        "-H", "Prefer: wait",
        "--data", &body_arg,
        &create_url,
    ]);
    let _ = std::fs::remove_file(&body_path);
    let mut prediction = created?;
    // Replicate error responses are HTTP-problem objects: {"title", "detail", "status": 401}
    // — note `status` is a NUMBER there, while a real prediction's `status` is a string
    // ("starting"/"succeeded"/...). Surface the human-readable detail, not a parse dead end.
    if prediction.get("status").map(|s| s.is_number()).unwrap_or(false)
        || (prediction.get("status").is_none() && prediction.get("detail").is_some())
    {
        let detail = prediction
            .get("detail")
            .and_then(|d| d.as_str())
            .or_else(|| prediction.get("title").and_then(|t| t.as_str()))
            .unwrap_or("unknown error");
        bail!("Replicate error: {detail}");
    }

    let started = Instant::now();
    loop {
        match prediction.get("status").and_then(|s| s.as_str()) {
            Some("succeeded") => break,
            Some("failed") | Some("canceled") => {
                let detail = prediction.get("error").and_then(|e| e.as_str()).unwrap_or("no detail");
                bail!("Replicate prediction failed: {detail}");
            }
            _ => {
                if started.elapsed() > PREDICTION_TIMEOUT {
                    bail!("Replicate prediction timed out after {}s", PREDICTION_TIMEOUT.as_secs());
                }
                let poll_url = prediction
                    .get("urls")
                    .and_then(|u| u.get("get"))
                    .and_then(|u| u.as_str())
                    .ok_or_else(|| anyhow!("Replicate response has no poll URL"))?
                    .to_string();
                std::thread::sleep(Duration::from_secs(2));
                prediction = curl_json(&["-H", &auth, &poll_url]).context("polling Replicate prediction")?;
            }
        }
    }

    let url = output_url(&prediction).ok_or_else(|| anyhow!("Replicate prediction has no output image"))?;
    let out = curl(&["-L", "-H", &auth, &url]).context("downloading enhanced image")?;
    if out.is_empty() {
        bail!("Replicate returned an empty image");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_settings_reads_key_and_model() {
        let cfg = parse_settings_ai(r#"{"aiApiKey": "r8_abc", "aiModel": "stability-ai/sdxl"}"#).unwrap();
        assert_eq!(cfg.api_key, "r8_abc");
        assert_eq!(cfg.model, "stability-ai/sdxl");
    }

    #[test]
    fn parse_settings_defaults_model_when_absent_or_blank() {
        let cfg = parse_settings_ai(r#"{"aiApiKey": "r8_abc"}"#).unwrap();
        assert_eq!(cfg.model, DEFAULT_MODEL);
        let cfg = parse_settings_ai(r#"{"aiApiKey": "r8_abc", "aiModel": "  "}"#).unwrap();
        assert_eq!(cfg.model, DEFAULT_MODEL);
    }

    #[test]
    fn parse_settings_treats_missing_or_empty_key_as_unconfigured() {
        assert!(parse_settings_ai(r#"{"gameExe": null}"#).is_none());
        assert!(parse_settings_ai(r#"{"aiApiKey": ""}"#).is_none());
        assert!(parse_settings_ai(r#"{"aiApiKey": "   "}"#).is_none());
        assert!(parse_settings_ai("not json").is_none());
    }

    #[test]
    fn texture_prompt_picks_category_from_real_library_names() {
        let wolf = texture_prompt("compiled/images/Animation/Monster/Ani_Monster_Wolf_Body_01_Diffuse_S1.png");
        assert!(wolf.contains("creature"), "{wolf}");
        let axe = texture_prompt("compiled/images/Special/ItWpn_Axes_01_Diffuse_01.png");
        assert!(axe.contains("metal"), "{axe}");
        let rock = texture_prompt("compiled/images/Nature/Nat_Stone_Rock_01_Diffuse_03.png");
        assert!(rock.contains("rock"), "{rock}");
        // every prompt carries the layout-preservation constraint — the one non-negotiable
        assert!(wolf.contains("EXACT same colors"));
    }

    #[test]
    fn data_maps_are_recognized() {
        assert!(is_data_map("compiled/images/Animation/Monster/Ani_Monster_Wolf_Body_01_Normal_S1.png"));
        assert!(is_data_map("Special/ItWpn_Axes_01_Specular_01.png"));
        assert!(!is_data_map("Special/ItWpn_Axes_01_Diffuse_01.png"));
    }

    #[test]
    fn esrgan_input_has_no_prompt_but_img2img_does() {
        let esrgan = build_input("nightmareai/real-esrgan", "data:image/png;base64,xxx", "a_Diffuse.png", 2);
        assert!(esrgan.get("prompt").is_none());
        assert_eq!(esrgan.get("scale").and_then(|v| v.as_u64()), Some(2));
        let sdxl = build_input("stability-ai/sdxl", "data:image/png;base64,xxx", "Monster_Wolf_Diffuse.png", 2);
        assert!(sdxl.get("prompt").and_then(|p| p.as_str()).unwrap().contains("creature"));
    }

    #[test]
    fn output_url_handles_string_and_array_shapes() {
        let s = serde_json::json!({"output": "https://x/img.png"});
        assert_eq!(output_url(&s).as_deref(), Some("https://x/img.png"));
        let a = serde_json::json!({"output": ["https://x/1.png", "https://x/2.png"]});
        assert_eq!(output_url(&a).as_deref(), Some("https://x/1.png"));
        let none = serde_json::json!({"status": "failed"});
        assert!(output_url(&none).is_none());
    }
}
