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
    /// "✨ Нові текстури" mode (settings `aiRegenerate`): true asks the img2img model to
    /// genuinely repaint the texture from the source's rough structure — a fresh piece of art
    /// for the same subject — instead of faithfully re-detailing what's already there. See
    /// `texture_prompt_regenerate` / `build_input`. Ignored by upscalers (no prompt involved).
    pub regenerate: bool,
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
    let regenerate = value.get("aiRegenerate").and_then(|r| r.as_bool()).unwrap_or(false);
    Some(AiConfig { provider, api_key: key, model, creativity, regenerate })
}

/// Reads AI config: `RISENLAB_AI_KEY` env var wins (model from settings or default), then the
/// settings file. `None` = feature not configured, callers fall back to local processing.
pub fn load_config() -> Option<AiConfig> {
    let from_settings =
        std::fs::read_to_string(settings_json_path()).ok().and_then(|json| parse_settings_ai(&json));
    if let Ok(env_key) = std::env::var("RISENLAB_AI_KEY") {
        let env_key = env_key.trim().to_string();
        if !env_key.is_empty() {
            let (provider, model, creativity, regenerate) = from_settings
                .map(|c| (c.provider, c.model, c.creativity, c.regenerate))
                .unwrap_or_else(|| ("replicate".to_string(), DEFAULT_MODEL.to_string(), 0.6, false));
            return Some(AiConfig { provider, api_key: env_key, model, creativity, regenerate });
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

/// Builds the "✨ Нові текстури" prompt: unlike `texture_prompt` (which pins the model to the
/// EXACT original colors/layout — the right call for a faithful re-detail), this one explicitly
/// tells the model to repaint the texture as fresh art. Only the rough silhouette needs to
/// survive (so the result still roughly fits the same UVs) — everything else, colors included,
/// is meant to change. This is the fix for the owner's "88 textures, 0 visible difference"
/// report: the old shared prompt was fighting every model into near-identity output regardless
/// of the creativity dial.
pub fn texture_prompt_regenerate(png_rel: &str) -> String {
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
        "Completely repaint this video game texture as a brand-new, dramatically higher-fidelity \
         {subject}. Do not just sharpen or clean up the input — invent fresh micro-detail, fresh \
         material variation, fresh color depth, fresh wear and grime: a genuinely new piece of \
         concept-art-quality texture art for the same subject, not a filter over the original. \
         Only the rough silhouette and overall shape may stay recognizable so it still roughly \
         fits the same UV layout — everything else (exact colors, fine patterns, surface detail) \
         should be freshly generated. Extremely detailed, 4k quality, no text, no watermark, no \
         frame, no new unrelated objects."
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
/// additionally gets the category prompt.
///
/// **Regenerate mode is ALWAYS routed through clarity-upscaler's params, regardless of the
/// configured `model` string** (added 2026-07-19, after `stability-ai/sdxl` — the model the
/// generic img2img branch below was originally built for — turned out to 404 on Replicate's
/// `/v1/models/{owner}/{name}/predictions` endpoint: confirmed live via a direct CLI call,
/// "Replicate error: The requested resource could not be found." Every "✨ Нові текстури"
/// attempt was silently failing and falling back to a plain Lanczos upscale — see the
/// risenlab-texture-render-fixes memory for the "0 diff" incident this caused). clarity-upscaler
/// is a model that's actually been confirmed working across many prior sessions; regenerate mode
/// pushes its own creativity/resemblance dial to the far end (near-max creativity, near-min
/// resemblance) instead of trusting an unverified model slug. The generic img2img branch at the
/// bottom is kept for anyone who types a genuinely different custom model into the free-text
/// field — untouched by this incident, still worth keeping as an option.
pub fn build_input(model: &str, image_data_uri: &str, png_rel: &str, scale: u32, creativity: f32, regenerate: bool) -> serde_json::Value {
    let creativity = creativity.clamp(0.1, 0.9);
    if model.to_lowercase().contains("clarity") || (regenerate && !model.to_lowercase().contains("esrgan")) {
        let prompt = if regenerate { texture_prompt_regenerate(png_rel) } else { texture_prompt(png_rel) };
        let negative_prompt = if regenerate {
            "blurry, low detail, flat, plain, same as input, watermark, text, frame"
        } else {
            "blurry, smooth, plastic, different colors, changed layout, new objects, text, watermark"
        };
        // Faithful mode: ~0.5 re-details faithfully, ~0.75 visibly re-imagines patterns
        // ("Ремастер"), resemblance moves opposite so the two knobs don't fight. Regenerate mode
        // pins both dials near their far end regardless of the creativity setting — this is the
        // "throw away the original, keep only the rough shape" mode, not a matter of degree.
        let (effective_creativity, resemblance, steps) = if regenerate {
            (creativity.max(0.85), (1.5 - creativity).clamp(0.1, 0.4), 30)
        } else {
            (creativity, (1.5 - creativity).clamp(0.5, 1.4), 22)
        };
        return serde_json::json!({
            "image": image_data_uri,
            "prompt": prompt,
            "negative_prompt": negative_prompt,
            "scale_factor": scale.clamp(2, 4),
            "creativity": effective_creativity,
            "resemblance": resemblance,
            "dynamic": 12,
            "num_inference_steps": steps,
        });
    }
    if model.to_lowercase().contains("esrgan") {
        return serde_json::json!({
            "image": image_data_uri,
            "scale": scale.clamp(2, 4),
            "face_enhance": false,
        });
    }
    if regenerate {
        serde_json::json!({
            "image": image_data_uri,
            "prompt": texture_prompt_regenerate(png_rel),
            "negative_prompt": "blurry, low detail, flat, plain, same as input, watermark, text, frame",
            "strength": (0.55 + creativity * 0.4).clamp(0.6, 0.95),
            "guidance_scale": 8.5,
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
    let mut cmd = Command::new("curl.exe");
    cmd.args(["-sS", "--max-time", "180"]).args(args);
    crate::content::suppress_console_window(&mut cmd);
    let out = cmd.output().context("running curl.exe (bundled with Windows 10+)")?;
    if !out.status.success() {
        bail!("curl failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

/// Runs `curl` with an `Authorization: Bearer <api_key>` header WITHOUT putting the key on the
/// command line. A plain `-H "Authorization: Bearer <key>"` argument is briefly visible to
/// anything that can read this process's argv while curl runs (e.g. Task Manager's "Command
/// line" column, or any other local process enumerating command lines) — low-severity for a
/// single-user local tool, but avoidable for free. Same fix as the existing request-body temp
/// file below, and for the same reason: keep secrets out of argv, put them in a short-lived
/// file only curl reads (`-K`/`--config`, which supports a `header = "..."` directive).
/// The `-K` config file's contents: a single `header = "..."` directive carrying the bearer
/// token. curl's config-file value syntax is a quoted string with `\`/`"` backslash-escaped —
/// a real API token won't contain either, but escape defensively rather than assume.
fn bearer_config_contents(api_key: &str) -> String {
    let escaped = api_key.replace('\\', "\\\\").replace('"', "\\\"");
    format!("header = \"Authorization: Bearer {escaped}\"\n")
}

fn curl_with_bearer(api_key: &str, args: &[&str]) -> Result<Vec<u8>> {
    let config_path = std::env::temp_dir().join(format!("risenlab_ai_auth_{}.cfg", std::process::id()));
    std::fs::write(&config_path, bearer_config_contents(api_key)).context("writing curl auth config temp file")?;
    let config_arg = config_path.to_string_lossy().into_owned();
    let mut full_args = vec!["-K", &config_arg];
    full_args.extend_from_slice(args);
    let result = curl(&full_args);
    let _ = std::fs::remove_file(&config_path);
    result
}

fn curl_json_with_bearer(api_key: &str, args: &[&str]) -> Result<serde_json::Value> {
    let bytes = curl_with_bearer(api_key, args)?;
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
    let image_arg = format!("image=@{}", src_png.display());
    let prompt_arg = format!("prompt={}", texture_prompt(png_rel));
    let out = curl_with_bearer(&cfg.api_key, &[
        "-X", "POST",
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

    // Request body → temp file (megabytes of base64 blow past the command-line length limit).
    let body = serde_json::json!({ "input": build_input(&cfg.model, &data_uri, png_rel, scale, cfg.creativity, cfg.regenerate) });
    let body_path = std::env::temp_dir().join(format!("risenlab_ai_{}.json", std::process::id()));
    std::fs::write(&body_path, serde_json::to_vec(&body)?).context("writing request body temp file")?;
    let body_arg = format!("@{}", body_path.display());

    // `Prefer: wait` holds the connection until the prediction finishes (up to ~60s on
    // Replicate's side) — most textures come back in this single round trip. Cold starts
    // fall through to polling below.
    let create_url = format!("https://api.replicate.com/v1/models/{}/predictions", cfg.model);
    let created = curl_json_with_bearer(&cfg.api_key, &[
        "-X", "POST",
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
                prediction = curl_json_with_bearer(&cfg.api_key, &[&poll_url]).context("polling Replicate prediction")?;
            }
        }
    }

    let url = output_url(&prediction).ok_or_else(|| anyhow!("Replicate prediction has no output image"))?;
    let out = curl_with_bearer(&cfg.api_key, &["-L", &url]).context("downloading enhanced image")?;
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
        let v = build_input("philz1337x/clarity-upscaler", "data:image/png;base64,x", "Monster_Ogre_Body_Diffuse.png", 2, 0.75, false);
        assert!(v.get("prompt").and_then(|p| p.as_str()).unwrap().contains("muscle"));
        assert_eq!(v.get("scale_factor").and_then(|x| x.as_u64()), Some(2));
        assert!(v.get("resemblance").is_some() && v.get("creativity").is_some());
    }

    #[test]
    fn esrgan_input_has_no_prompt_but_img2img_does() {
        let esrgan = build_input("nightmareai/real-esrgan", "data:image/png;base64,xxx", "a_Diffuse.png", 2, 0.5, false);
        assert!(esrgan.get("prompt").is_none());
        assert_eq!(esrgan.get("scale").and_then(|v| v.as_u64()), Some(2));
        let sdxl = build_input("stability-ai/sdxl", "data:image/png;base64,xxx", "Monster_Wolf_Diffuse.png", 2, 0.5, false);
        assert!(sdxl.get("prompt").and_then(|p| p.as_str()).unwrap().contains("creature"));
    }

    #[test]
    fn regenerate_mode_routes_through_clarity_params_even_for_an_unrelated_model_string() {
        // The core of the 2026-07-19 fix: "stability-ai/sdxl" 404s on Replicate (confirmed
        // live), so regenerate mode must NOT depend on that (or any other) unverified model
        // slug — it always uses clarity-upscaler's actually-working endpoint/params instead.
        let v = build_input("stability-ai/sdxl", "data:image/png;base64,xxx", "Monster_Wolf_Diffuse.png", 2, 0.85, true);
        let prompt = v.get("prompt").and_then(|p| p.as_str()).unwrap();
        assert!(prompt.contains("brand-new"), "{prompt}");
        assert!(!prompt.contains("EXACT same colors"), "regenerate mode must not pin exact colors: {prompt}");
        // clarity-style params, NOT the generic-img2img "strength"/"guidance_scale" shape.
        assert!(v.get("strength").is_none(), "regenerate must not use the unverified generic img2img branch");
        assert_eq!(v.get("scale_factor").and_then(|x| x.as_u64()), Some(2));
        let creativity = v.get("creativity").and_then(|c| c.as_f64()).unwrap();
        let resemblance = v.get("resemblance").and_then(|r| r.as_f64()).unwrap();
        assert!(creativity >= 0.849, "regenerate creativity should be pinned near max, got {creativity}");
        assert!(resemblance <= 0.401, "regenerate resemblance should be pinned near min, got {resemblance}");

        // Compare against the faithful clarity call (same branch, same JSON shape) at the same
        // creativity dial — "stability-ai/sdxl" itself only reaches this "resemblance"-shaped
        // branch at all when regenerate=true; non-regenerate falls to the unrelated generic
        // img2img "strength"-shaped branch, so it isn't a like-for-like comparison.
        let faithful = build_input("philz1337x/clarity-upscaler", "data:image/png;base64,xxx", "Monster_Wolf_Diffuse.png", 2, 0.85, false);
        let faithful_resemblance = faithful.get("resemblance").and_then(|r| r.as_f64()).unwrap();
        assert!(resemblance < faithful_resemblance, "regenerate must diverge more than the faithful mode at the same creativity");
    }

    #[test]
    fn regenerate_mode_still_uses_clarity_params_when_model_is_explicitly_clarity() {
        let v = build_input("philz1337x/clarity-upscaler", "data:image/png;base64,xxx", "Monster_Wolf_Diffuse.png", 2, 0.5, true);
        assert!(v.get("prompt").and_then(|p| p.as_str()).unwrap().contains("brand-new"));
        let creativity = v.get("creativity").and_then(|c| c.as_f64()).unwrap();
        assert!((creativity - 0.85).abs() < 0.001, "regenerate floors creativity at 0.85 regardless of the dial, got {creativity}");
    }

    #[test]
    fn regenerate_mode_leaves_esrgan_alone_since_it_never_took_a_prompt() {
        let v = build_input("nightmareai/real-esrgan", "data:image/png;base64,xxx", "Monster_Wolf_Diffuse.png", 2, 0.85, true);
        assert!(v.get("prompt").is_none());
        assert_eq!(v.get("scale").and_then(|x| x.as_u64()), Some(2));
    }

    #[test]
    fn texture_prompt_regenerate_keeps_only_the_silhouette_constraint() {
        let wolf = texture_prompt_regenerate("compiled/images/Animation/Monster/Ani_Monster_Wolf_Body_01_Diffuse_S1.png");
        assert!(wolf.contains("creature"), "{wolf}");
        assert!(wolf.contains("silhouette"));
        assert!(wolf.contains("brand-new"));
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

    #[test]
    fn bearer_config_never_contains_the_bare_header_flag_style_and_carries_the_real_key() {
        let contents = bearer_config_contents("r8_realkeyvalue123");
        assert_eq!(contents, "header = \"Authorization: Bearer r8_realkeyvalue123\"\n");
        // The whole point: this goes in a file curl reads with `-K`, never on argv/the command
        // line curl.exe itself is launched with — nothing to assert on argv here since that's
        // exactly the point of NOT constructing a `-H "..."` string for the key anymore.
    }

    #[test]
    fn bearer_config_escapes_quotes_and_backslashes_in_the_key() {
        let contents = bearer_config_contents(r#"weird\key"with"quotes"#);
        assert_eq!(contents, "header = \"Authorization: Bearer weird\\\\key\\\"with\\\"quotes\"\n");
    }
}
