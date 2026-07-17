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
    /// "replicate" (default) or "stability". OpenAI images is deliberately NOT offered: its
    /// output comes only in fixed sizes (1024/1536), which breaks a texture's aspect/UVs.
    pub provider: String,
    pub api_key: String,
    /// `owner/name` on Replicate, e.g. "nightmareai/real-esrgan". Unused by Stability (its
    /// conservative-upscale endpoint is fixed).
    pub model: String,
    /// 0.1..0.9, the "how much may the AI invent" dial (settings `aiCreativity`): clarity's
    /// creativity / SDXL's denoising strength. The UI's modes are presets over this —
    /// «Деталізований» ≈ 0.5, «Ремастер» ≈ 0.75.
    pub creativity: f32,
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
    let provider = value
        .get("aiProvider")
        .and_then(|p| p.as_str())
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "replicate".to_string());
    let creativity = value
        .get("aiCreativity")
        .and_then(|c| c.as_f64())
        .map(|c| (c as f32).clamp(0.1, 0.9))
        .unwrap_or(0.6);
    Some(AiConfig { provider, api_key: key, model, creativity })
}

/// Reads AI config: `RISENLAB_AI_KEY` env var wins (model from settings or default), then the
/// settings file. `None` = feature not configured, callers fall back to local processing.
pub fn load_config() -> Option<AiConfig> {
    let from_settings =
        std::fs::read_to_string(settings_json_path()).ok().and_then(|json| parse_settings_ai(&json));
    if let Ok(env_key) = std::env::var("RISENLAB_AI_KEY") {
        let env_key = env_key.trim().to_string();
        if !env_key.is_empty() {
            let (provider, model, creativity) = from_settings
                .map(|c| (c.provider, c.model, c.creativity))
                .unwrap_or_else(|| ("replicate".to_string(), DEFAULT_MODEL.to_string(), 0.6));
            return Some(AiConfig { provider, api_key: env_key, model, creativity });
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
         Extremely detailed micro-surface: pronounced muscle definition, skin pores, \
         individual fur strands, fabric weave, metal scratches and stone grain where the \
         material calls for it — crisp, never smoothed or plastic-looking. Keep the EXACT \
         same colors, layout, silhouette and UV boundaries as the original image, seamless \
         where the original is seamless, no new objects, no text, 4k quality."
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
pub fn build_input(model: &str, image_data_uri: &str, png_rel: &str, scale: u32, creativity: f32) -> serde_json::Value {
    let creativity = creativity.clamp(0.1, 0.9);
    if model.to_lowercase().contains("clarity") {
        // philz1337x/clarity-upscaler — an upscaler that ADDS detail (tiled SD guided by the
        // prompt) instead of real-esrgan's smoothing. `creativity` is the mode dial: ~0.5
        // re-details faithfully, ~0.75 visibly re-imagines patterns ("Ремастер" mode);
        // resemblance moves opposite so the two knobs don't fight each other.
        return serde_json::json!({
            "image": image_data_uri,
            "prompt": texture_prompt(png_rel),
            "negative_prompt": "blurry, smooth, plastic, different colors, changed layout, new objects, text, watermark",
            "scale_factor": scale.clamp(2, 4),
            "creativity": creativity,
            "resemblance": (1.5 - creativity).clamp(0.5, 1.4),
            "dynamic": 12,
            "num_inference_steps": 22,
        });
    }
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
            "strength": (creativity * 0.7).clamp(0.15, 0.6),
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

/// Runs one real enhancement through the configured provider and returns the enhanced image
/// bytes (png/jpeg/webp — caller decodes). Blocking by design: this is a CLI.
pub fn enhance_png(cfg: &AiConfig, src_png: &Path, png_rel: &str, scale: u32) -> Result<Vec<u8>> {
    match cfg.provider.as_str() {
        "replicate" => enhance_via_replicate(cfg, src_png, png_rel, scale),
        "stability" => enhance_via_stability(cfg, src_png, png_rel),
        other => bail!("unknown AI provider '{other}' (expected replicate|stability)"),
    }
}

/// Stability AI's conservative upscale (`/v2beta/stable-image/upscale/conservative`): one
/// multipart POST, image bytes come straight back (no polling). "Conservative" is exactly the
/// texture-safe mode — content/colors preserved, up to ~4x. Prompt is a required field there;
/// the per-category texture prompt serves as it. Errors come back as JSON (starts with '{'),
/// success as raw image bytes — that's the discriminator.
fn enhance_via_stability(cfg: &AiConfig, src_png: &Path, png_rel: &str) -> Result<Vec<u8>> {
    let auth = format!("Authorization: Bearer {}", cfg.api_key);
    let image_arg = format!("image=@{}", src_png.display());
    let prompt_arg = format!("prompt={}", texture_prompt(png_rel));
    let out = curl(&[
        "-X", "POST",
        "-H", &auth,
        "-H", "Accept: image/*",
        "-F", &image_arg,
        "-F", &prompt_arg,
        "-F", "output_format=png",
        "https://api.stability.ai/v2beta/stable-image/upscale/conservative",
    ])?;
    if out.first() == Some(&b'{') {
        bail!("Stability error: {}", String::from_utf8_lossy(&out[..out.len().min(400)]));
    }
    if out.is_empty() {
        bail!("Stability returned an empty image");
    }
    Ok(out)
}

fn enhance_via_replicate(cfg: &AiConfig, src_png: &Path, png_rel: &str, scale: u32) -> Result<Vec<u8>> {
    let bytes = std::fs::read(src_png).with_context(|| format!("reading {}", src_png.display()))?;
    let data_uri = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes));
    let auth = format!("Authorization: Bearer {}", cfg.api_key);

    // Request body → temp file (megabytes of base64 blow past the command-line length limit).
    let body = serde_json::json!({ "input": build_input(&cfg.model, &data_uri, png_rel, scale, cfg.creativity) });
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
        assert_eq!(cfg.provider, "replicate", "provider defaults to replicate");
    }

    #[test]
    fn parse_settings_reads_provider() {
        let cfg = parse_settings_ai(r#"{"aiApiKey": "sk-abc", "aiProvider": "Stability"}"#).unwrap();
        assert_eq!(cfg.provider, "stability", "normalized to lowercase");
        let cfg = parse_settings_ai(r#"{"aiApiKey": "k", "aiProvider": ""}"#).unwrap();
        assert_eq!(cfg.provider, "replicate", "empty falls back to default");
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
    fn clarity_input_carries_prompt_and_faithfulness_knobs() {
        let v = build_input("philz1337x/clarity-upscaler", "data:image/png;base64,x", "Monster_Ogre_Body_Diffuse.png", 2, 0.75);
        assert!(v.get("prompt").and_then(|p| p.as_str()).unwrap().contains("muscle"));
        assert_eq!(v.get("scale_factor").and_then(|x| x.as_u64()), Some(2));
        assert!(v.get("resemblance").is_some() && v.get("creativity").is_some());
    }

    #[test]
    fn esrgan_input_has_no_prompt_but_img2img_does() {
        let esrgan = build_input("nightmareai/real-esrgan", "data:image/png;base64,xxx", "a_Diffuse.png", 2, 0.5);
        assert!(esrgan.get("prompt").is_none());
        assert_eq!(esrgan.get("scale").and_then(|v| v.as_u64()), Some(2));
        let sdxl = build_input("stability-ai/sdxl", "data:image/png;base64,xxx", "Monster_Wolf_Diffuse.png", 2, 0.5);
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
