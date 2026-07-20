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

/// Builds an `AiConfig` from already-typed field values (defaulting/clamping rules shared with
/// `parse_settings_ai` below) — the packaged Tauri app's settings are ALREADY a parsed
/// `AppSettings` struct (`app/src-tauri/src/logic.rs`), not JSON text, so it has no reason to
/// round-trip through `serde_json::Value` just to reach this same logic. An empty/whitespace key
/// counts as "not configured", same as the JSON path.
pub fn config_from_parts(provider: Option<&str>, api_key: &str, model: Option<&str>, creativity: Option<f32>, regenerate: bool) -> Option<AiConfig> {
    let key = api_key.trim().to_string();
    if key.is_empty() {
        return None;
    }
    let model = model.map(str::trim).filter(|m| !m.is_empty()).unwrap_or(DEFAULT_MODEL).to_string();
    let provider = provider
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "replicate".to_string());
    let creativity = creativity.map(|c| c.clamp(0.1, 0.9)).unwrap_or(0.6);
    Some(AiConfig { provider, api_key: key, model, creativity, regenerate })
}

/// Extracts `(api_key, model)` from the settings JSON text. Separated from file I/O so the
/// parsing is unit-testable. An empty/whitespace key counts as "not configured".
pub fn parse_settings_ai(json: &str) -> Option<AiConfig> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let key = value.get("aiApiKey")?.as_str()?;
    let model = value.get("aiModel").and_then(|m| m.as_str());
    let provider = value.get("aiProvider").and_then(|p| p.as_str());
    let creativity = value.get("aiCreativity").and_then(|c| c.as_f64()).map(|c| c as f32);
    let regenerate = value.get("aiRegenerate").and_then(|r| r.as_bool()).unwrap_or(false);
    config_from_parts(provider, key, model, creativity, regenerate)
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

/// Known non-name tokens in this game's real actor/texture filenames — everything a creature
/// name is never spelled as, so scanning past them finds the real name. Kept intentionally
/// narrow (exact tokens, not substrings) so a real creature name never accidentally matches one.
const NAME_SKIP_TOKENS: &[&str] = &[
    "ani", "hero", "monster", "object", "it", "body", "comp", "composite", "claws", "claw", "eyes", "eye", "head",
    "diffuse", "normal", "normalmap", "specular", "billboard", "billboards",
];

/// Best-guess creature/character name pulled straight out of the texture's own real filename
/// (e.g. "Ani_Monster_Wolf_Body_01_Diffuse_S1.png" → "Wolf", "Ani_Monster_Body_Nautilus_01_
/// Diffuse_S1.png" → "Nautilus" — the name isn't always right after "Monster"; body-part words
/// can come first). Told to the AI so it knows *which* creature it's re-texturing instead of a
/// generic "a fantasy monster" — the owner's "щоб він розумів, що текстура яку він опрацьовує
/// - це вовк" request. `None` when the filename doesn't look like a real actor/monster texture
/// (nothing lost — the category-only subject line still applies).
fn guess_creature_name(png_rel: &str) -> Option<String> {
    let stem = Path::new(png_rel).file_stem()?.to_str()?.to_string();
    let tokens: Vec<&str> = stem.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).collect();
    let lower: Vec<String> = tokens.iter().map(|t| t.to_lowercase()).collect();
    let monster_pos = lower.iter().position(|t| t == "monster")?;
    tokens[monster_pos + 1..]
        .iter()
        .zip(lower[monster_pos + 1..].iter())
        .find(|(_, l)| {
            !NAME_SKIP_TOKENS.contains(&l.as_str())
                && l.len() > 1
                && !(l.starts_with('s') && l[1..].chars().all(|c| c.is_ascii_digit()))
                && !l.chars().all(|c| c.is_ascii_digit())
        })
        .map(|(orig, _)| orig.to_string())
}

/// Category-word guess from the real naming conventions in the game's own library — shared by
/// both prompt builders below. `None` when nothing matches (each caller supplies its own
/// fallback text: the faithful and regenerate prompts intentionally use different ones).
fn texture_category(lower: &str) -> Option<&'static str> {
    if lower.contains("monster") || lower.contains("wolf") || lower.contains("ogre") {
        Some("creature skin, fur and hide of a fantasy monster")
    } else if lower.contains("head") || lower.contains("face") || lower.contains("hero") || lower.contains("npc") {
        Some("realistic human skin, face and hair")
    } else if lower.contains("armor") || lower.contains("wpn") || lower.contains("weapon") || lower.contains("sword") || lower.contains("axe") {
        Some("weathered metal, steel and leather of medieval weapons and armor")
    } else if lower.contains("nat_") || lower.contains("stone") || lower.contains("rock") || lower.contains("tree") || lower.contains("plant") {
        Some("natural rock, stone, bark and foliage surfaces")
    } else if lower.contains("arch") || lower.contains("wall") || lower.contains("house") || lower.contains("building") {
        Some("medieval architecture: plaster, brick, carved stone and timber")
    } else if lower.contains("cloth") || lower.contains("cape") || lower.contains("robe") {
        Some("woven cloth, linen and rough medieval fabric")
    } else {
        None
    }
}

/// `texture_category`'s match, with the real creature name (`guess_creature_name`) folded in
/// when this is a monster texture — "щоб він розумів, що текстура яку він опрацьовує - це вовк".
/// Falls back to `fallback` when no category matched at all.
fn texture_subject(png_rel: &str, fallback: &str) -> String {
    let lower = png_rel.to_lowercase();
    let category = texture_category(&lower).unwrap_or(fallback);
    match guess_creature_name(png_rel) {
        Some(name) if lower.contains("monster") => format!("{category} — specifically a {name}-type creature"),
        _ => category.to_string(),
    }
}

/// Shared safety guardrails, appended to BOTH prompts below — the fix for the owner's
/// "Франкенштейн" report (real screenshots, 2026-07-19): several real diffuse maps are TEXTURE
/// ATLASES, several unrelated material crops (body/claws/eyes) tiled on a plain/black
/// background, not one coherent photo. Handed a category word like "creature skin" and no
/// warning about that layout, the model interpreted the blobby, low-detail crops as an
/// unfinished portrait and "completed" it: real result was a wall of invented monster HEADS
/// with eyes/mouths/faces, extra small creatures filling the black background, and — on one
/// actual eye crop — an unrelated abstract stained-glass pattern instead of a plain eye. None
/// of that exists in the source; this is the model's own prior for "creature" filling in the
/// gaps, not a repaint of what's actually there. This explicitly tells it not to. Applies to
/// every model/branch (`build_input` puts it in the prompt for both faithful and regenerate
/// modes, and the matching negative-prompt phrases below).
const ATLAS_AND_REALISM_GUARDRAILS: &str = "\
This image is source material for a real video-game texture remaster — a UV-mapped surface \
asset, not concept art, a poster, or a character illustration. It may be a texture ATLAS: \
several separate, disconnected material crops (e.g. body/claws/eyes) tiled on a plain or black \
background, each its own independent region, not one coherent photo or scene. If so, repaint \
each region's own material in place and leave the plain/black background between them empty — \
never merge separate regions into one connected image, and never add new content to fill the \
gaps between them. Do not invent a face, eyes, mouth, or head that isn't already present in the \
source — a blurry limb, patch of hide, or color blob stays exactly that, not a creature \
portrait. Do not add extra creatures, characters, or figures anywhere in the image.";

const ATLAS_NEGATIVE_TERMS: &str =
    "invented face, invented eyes, creature portrait, character illustration, bestiary, poster, collage of unrelated creatures, extra heads, extra figures, humanoid silhouette, connected scene across separate regions";

/// Builds the per-texture text prompt for img2img-style models, derived from the texture's
/// own library name/path (e.g. "compiled/images/Animation/Monster/Ani_Monster_Wolf_Body_01_
/// Diffuse_S1.png"). Category keywords come from the real naming conventions in the game's
/// own library. Upscaler models ignore prompts entirely — harmless to compute either way.
pub fn texture_prompt(png_rel: &str) -> String {
    let subject = texture_subject(png_rel, "game asset surface material");
    format!(
        "High-resolution remaster of a video game texture: {subject}. \
         Extremely detailed micro-surface: pronounced muscle definition, skin pores, \
         individual fur strands, fabric weave, metal scratches and stone grain where the \
         material calls for it — crisp, never smoothed or plastic-looking. Keep the EXACT \
         same colors, layout, silhouette and UV boundaries as the original image, seamless \
         where the original is seamless, no new objects, no text, 4k quality. \
         {ATLAS_AND_REALISM_GUARDRAILS}"
    )
}

/// Full-scene concept art (loading screens, menu backgrounds) is genuinely one coherent
/// illustrated scene, not an atlas of separate material crops — the one real exception to the
/// atlas guardrails, in both the prompt (`texture_prompt_regenerate`) and the negative prompt
/// (`build_input`), which is why this is shared between the two.
fn is_full_scene_texture(png_rel: &str) -> bool {
    let lower = png_rel.to_lowercase();
    lower.contains("gui") || lower.contains("loadinghint") || lower.contains("menu") || lower.contains("splash")
}

/// Builds the "✨ Нові текстури" prompt: unlike `texture_prompt` (which pins the model to the
/// EXACT original colors/layout — the right call for a faithful re-detail), this one explicitly
/// tells the model to repaint the texture as fresh art. Only the rough silhouette needs to
/// survive (so the result still roughly fits the same UVs) — everything else, colors included,
/// is meant to change. This is the fix for the owner's "88 textures, 0 visible difference"
/// report: the old shared prompt was fighting every model into near-identity output regardless
/// of the creativity dial.
pub fn texture_prompt_regenerate(png_rel: &str) -> String {
    let is_full_scene = is_full_scene_texture(png_rel);
    let subject = if is_full_scene {
        // A "surface material" description here (the old generic fallback) told the model this
        // was a flat texture swatch, which combined with a too-high strength produced a
        // completely unrelated fantasy illustration instead of a repaint of the actual scene
        // (real incident, 2026-07-19).
        "dark fantasy concept-art illustration — the same characters, objects and scene composition as the input, painted with dramatically richer detail".to_string()
    } else {
        texture_subject(png_rel, "photorealistic game asset material, matching the input's own real subject matter")
    };
    let guardrails = if is_full_scene { "" } else { ATLAS_AND_REALISM_GUARDRAILS };
    format!(
        "Completely repaint this video game image as a brand-new, dramatically higher-fidelity \
         {subject}. Do not just sharpen or clean up the input — invent fresh micro-detail, fresh \
         material variation, fresh color depth, fresh wear and grime: a genuinely new piece of \
         concept-art-quality art. This MUST still depict the same subject, the same objects and \
         the same overall composition as the input image — a repaint of THIS specific picture, \
         never a different, unrelated scene or character. Only the exact colors and fine surface \
         detail should be freshly generated; the silhouette, pose, layout and subject identity \
         must stay recognizable. Extremely detailed, 4k quality, no text, no watermark, no frame, \
         no new unrelated objects, no different character. \
         {guardrails}"
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
/// **Regenerate mode does NOT force clarity-upscaler** (tried that briefly on 2026-07-19, see
/// git history — reverted the same day). clarity-upscaler is a *tiled* diffusion upscaler (its
/// own docs: "Tiled Diffusion... Tile count: 4"): at low creativity the tiles stay coherent with
/// each other and the source, which is exactly why it makes a good faithful "Ремастер" mode —
/// but pushed to near-max creativity/near-min resemblance to force a real reimagine, each tile
/// hallucinates almost independently, producing an incoherent grid of unrelated content (real
/// example: an ogre portrait became a collage of unrelated sci-fi faces). Wrong tool for "throw
/// away the original, keep the rough shape" — that needs a normal *global* (non-tiled) img2img
/// model, which the generic branch below already targets. The REAL, final root cause of the
/// whole "AI regenerate does nothing" saga was `resolve_model_version` (see its doc comment) —
/// once that's fixed, any real model (clarity or a normal SDXL img2img model) works via its own
/// natural branch below; no model needs special-casing.
pub fn build_input(model: &str, image_data_uri: &str, png_rel: &str, scale: u32, creativity: f32, regenerate: bool) -> serde_json::Value {
    let creativity = creativity.clamp(0.1, 0.9);
    // See ATLAS_NEGATIVE_TERMS's doc comment (on ATLAS_AND_REALISM_GUARDRAILS) — skipped for
    // full-scene concept art (loading screens/menus), which can legitimately contain characters.
    let negative_suffix = if is_full_scene_texture(png_rel) { String::new() } else { format!(", {ATLAS_NEGATIVE_TERMS}") };
    if model.to_lowercase().contains("clarity") {
        // philz1337x/clarity-upscaler — an upscaler that ADDS detail (tiled SD guided by the
        // prompt) instead of real-esrgan's smoothing. `creativity` is the mode dial: ~0.5
        // re-details faithfully, ~0.75 visibly re-imagines patterns ("Ремастер" mode);
        // resemblance moves opposite so the two knobs don't fight each other. Regenerate mode
        // nudges both further but stays well short of the incoherent-tile-collage cliff (see the
        // function doc comment) — this is the "someone explicitly typed clarity into the model
        // field" path, not the default "✨ Нові текстури" one.
        let prompt = if regenerate { texture_prompt_regenerate(png_rel) } else { texture_prompt(png_rel) };
        let (effective_creativity, resemblance) = if regenerate {
            (creativity.max(0.8), (1.5 - creativity).clamp(0.35, 0.6))
        } else {
            (creativity, (1.5 - creativity).clamp(0.5, 1.4))
        };
        return serde_json::json!({
            "image": image_data_uri,
            "prompt": prompt,
            "negative_prompt": format!("blurry, smooth, plastic, different colors, changed layout, new objects, text, watermark{negative_suffix}"),
            "scale_factor": scale.clamp(2, 4),
            "creativity": effective_creativity,
            "resemblance": resemblance,
            "dynamic": 12,
            "num_inference_steps": 22,
        });
    }
    if model.to_lowercase().contains("esrgan") {
        return serde_json::json!({
            "image": image_data_uri,
            "scale": scale.clamp(2, 4),
            "face_enhance": false,
        });
    }
    if model.to_lowercase().contains("supir") {
        // SUPIR (real model checked live against Replicate's metadata endpoint, 2026-07-20 —
        // owner report: "чому фото не генерує" after typing shanginn/supir into the model
        // field) has a COMPLETELY different input schema from every other model this app talks
        // to: `image`/`captions`/`n_prompt`/`upscale`/`s_cfg`/`s_churn`/`s_noise`/`edm_steps`/…
        // — nothing named `prompt`/`negative_prompt`/`strength`/`guidance_scale`. Falling
        // through to the generic branch below would silently send fields SUPIR doesn't
        // recognize, at best wasting the owner's prompt/creativity settings (SUPIR would just
        // run on its own defaults) and at worst getting rejected outright by its schema
        // validation. Map onto SUPIR's REAL fields instead: `captions` is its actual scene-
        // description input (its own default: "a professional, detailed, high-quality photo"),
        // `n_prompt` is its negative prompt. Every other SUPIR-specific knob (s_cfg/s_churn/
        // s_noise/edm_steps/color_fix_type/upscale...) is left at SUPIR's own tuned defaults —
        // this is a heavy, slow, real-compute model, and guessing a "creativity" mapping onto
        // knobs nobody here has empirically validated risks wasting the owner's Replicate credit
        // on a bad guess (unlike clarity-upscaler's creativity/resemblance dial, which WAS
        // live-validated against the owner's own reference, see this function's own history).
        let prompt = if regenerate { texture_prompt_regenerate(png_rel) } else { texture_prompt(png_rel) };
        return serde_json::json!({
            "image": image_data_uri,
            "captions": prompt,
            "n_prompt": format!("blurring, dirty, messy, worst quality, low quality, frames, watermark, signature, jpeg artifacts, deformed, lowres, over-smooth{negative_suffix}"),
        });
    }
    if regenerate {
        // Real incident (2026-07-19): strength this close to 1.0 makes SDXL-family img2img
        // models ignore the input image almost entirely — the "AI variant" came back as a
        // completely unrelated fantasy-warrior illustration with zero connection to the actual
        // source (an ogre portrait), because at ~0.9 strength there's essentially nothing left
        // of the original for the model to build on. Capped well below that so the model still
        // treats the input as its starting point, not a hint.
        serde_json::json!({
            "image": image_data_uri,
            "prompt": texture_prompt_regenerate(png_rel),
            "negative_prompt": format!("blurry, low detail, flat, plain, same as input, watermark, text, frame{negative_suffix}"),
            "strength": (0.45 + creativity * 0.3).clamp(0.45, 0.7),
            "guidance_scale": 8.5,
        })
    } else {
        serde_json::json!({
            "image": image_data_uri,
            "prompt": texture_prompt(png_rel),
            "negative_prompt": format!("different colors, changed layout, new objects, text, watermark, frame, blurry{negative_suffix}"),
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

/// Resolves `owner/name` to its current default version id via Replicate's model metadata
/// endpoint (a free, read-only GET — no prediction/compute cost, doesn't touch the owner's
/// credit). **Necessary, not optional**: the shorthand `POST /v1/models/{owner}/{name}/predictions`
/// endpoint only works for a subset of models — confirmed empirically (2026-07-19) that
/// `philz1337x/clarity-upscaler` (a real, public, 29.8M-run model) 404s
/// ("The requested resource could not be found") on that shortcut despite existing and being
/// perfectly reachable via this metadata endpoint. This was the actual, final root cause of an
/// extended "AI regenerate does nothing" incident — every non-real-esrgan model silently 404'd
/// and fell back to a plain Lanczos upscale, previously misdiagnosed as low Replicate credit
/// (a real, separate, simultaneously-true issue) and as a bad model choice. The classic
/// `POST /v1/predictions` endpoint with an explicit `"version"` hash works universally.
fn resolve_model_version(api_key: &str, model: &str) -> Result<String> {
    let url = format!("https://api.replicate.com/v1/models/{model}");
    let info = curl_json_with_bearer(api_key, &[&url]).with_context(|| format!("looking up Replicate model {model}"))?;
    info.get("latest_version")
        .and_then(|v| v.get("id"))
        .and_then(|id| id.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow!("Replicate model '{model}' has no latest_version — check the owner/name is correct and the model is public"))
}

/// SDXL (and similar generation-first, non-tiled img2img models) run out of GPU memory on a
/// full-resolution game texture: confirmed live (2026-07-19) — a 2048x2048 input to
/// `stability-ai/sdxl` failed with "CUDA out of memory. Tried to allocate 16.00 GiB". Upscalers
/// (real-esrgan, clarity-upscaler) are built to tile arbitrarily large images and don't hit
/// this — only the generic-img2img "regenerate" path needs the source downscaled first. The
/// output comes back near this capped size, which the caller's own local Lanczos step downstream
/// (see `batch::regenerate`) already exists to size back up to the texture's real target.
const IMG2IMG_MAX_EDGE: u32 = 1024;

fn downscale_for_img2img(bytes: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(bytes).context("decoding source PNG for img2img downscale")?;
    let (w, h) = (img.width(), img.height());
    if w.max(h) <= IMG2IMG_MAX_EDGE {
        return Ok(bytes.to_vec());
    }
    let scale = IMG2IMG_MAX_EDGE as f32 / w.max(h) as f32;
    let (new_w, new_h) = ((w as f32 * scale).round().max(1.0) as u32, (h as f32 * scale).round().max(1.0) as u32);
    let resized = image::imageops::resize(&img, new_w, new_h, image::imageops::FilterType::Lanczos3);
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(resized)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .context("re-encoding downscaled img2img source")?;
    Ok(out)
}

fn enhance_via_replicate(cfg: &AiConfig, src_png: &Path, png_rel: &str, scale: u32) -> Result<Vec<u8>> {
    let bytes = std::fs::read(src_png).with_context(|| format!("reading {}", src_png.display()))?;
    let (orig_w, orig_h) = image::image_dimensions(src_png).with_context(|| format!("reading dimensions of {}", src_png.display()))?;
    // Only the generic img2img branch (non-clarity, non-esrgan) needs this — see
    // IMG2IMG_MAX_EDGE's doc comment.
    // SUPIR is itself a restoration/upscale model (like clarity/esrgan, unlike a generic SDXL
    // img2img refiner) — it's built to take large inputs directly, not something that needs
    // downscaling to dodge GPU OOM (see IMG2IMG_MAX_EDGE's doc comment, which is specifically
    // about SDXL-family models running out of memory on a full-res texture).
    let is_generic_img2img =
        !cfg.model.to_lowercase().contains("clarity") && !cfg.model.to_lowercase().contains("esrgan") && !cfg.model.to_lowercase().contains("supir");
    let bytes = if is_generic_img2img { downscale_for_img2img(&bytes)? } else { bytes };
    let data_uri = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes));

    let version = resolve_model_version(&cfg.api_key, &cfg.model)?;

    // Request body → temp file (megabytes of base64 blow past the command-line length limit).
    let body = serde_json::json!({
        "version": version,
        "input": build_input(&cfg.model, &data_uri, png_rel, scale, cfg.creativity, cfg.regenerate),
    });
    let body_path = std::env::temp_dir().join(format!("risenlab_ai_{}.json", std::process::id()));
    std::fs::write(&body_path, serde_json::to_vec(&body)?).context("writing request body temp file")?;
    let body_arg = format!("@{}", body_path.display());

    // `Prefer: wait` holds the connection until the prediction finishes (up to ~60s on
    // Replicate's side) — most textures come back in this single round trip. Cold starts
    // fall through to polling below. The classic (non-shorthand) endpoint — see
    // `resolve_model_version`'s doc comment for why the shorthand isn't used here.
    let create_url = "https://api.replicate.com/v1/predictions".to_string();
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
    // The generic img2img model returns its own native resolution (e.g. SDXL's ~1024px),
    // which can be SMALLER than the original texture (downscaled going in, see
    // `downscale_for_img2img`) — upscale back up to at least the original size locally so the
    // patched texture isn't a downgrade in-game. Upscalers (clarity/esrgan) already return
    // `scale`x the original and never hit this.
    if is_generic_img2img {
        if let Ok(result_img) = image::load_from_memory(&out) {
            let (rw, rh) = (result_img.width(), result_img.height());
            if rw < orig_w || rh < orig_h {
                let target_w = orig_w.max(rw);
                let target_h = orig_h.max(rh);
                let resized = image::imageops::resize(&result_img, target_w, target_h, image::imageops::FilterType::Lanczos3);
                let mut upscaled = Vec::new();
                if image::DynamicImage::ImageRgba8(resized)
                    .write_to(&mut std::io::Cursor::new(&mut upscaled), image::ImageFormat::Png)
                    .is_ok()
                {
                    return Ok(upscaled);
                }
            }
        }
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
    fn config_from_parts_matches_parse_settings_ai_on_the_same_values() {
        // The real fix for "яку б модель я не вибирав - генерації не відбувається" (2026-07-20):
        // the packaged Tauri app's real settings are an already-parsed `AppSettings` struct, not
        // JSON text — this must produce IDENTICAL `AiConfig`s to the JSON path for the same
        // logical values, or the packaged app and the CLI/dev-bridge would silently disagree on
        // what "configured" means.
        let from_json = parse_settings_ai(r#"{"aiApiKey": "r8_abc", "aiModel": "stability-ai/sdxl", "aiProvider": "Stability", "aiCreativity": 0.85, "aiRegenerate": true}"#).unwrap();
        let from_parts = config_from_parts(Some("Stability"), "r8_abc", Some("stability-ai/sdxl"), Some(0.85), true).unwrap();
        assert_eq!(from_json, from_parts);
    }

    #[test]
    fn config_from_parts_treats_missing_or_empty_key_as_unconfigured() {
        assert!(config_from_parts(None, "", None, None, false).is_none());
        assert!(config_from_parts(None, "   ", None, None, false).is_none());
    }

    #[test]
    fn config_from_parts_defaults_model_and_provider_when_absent() {
        let cfg = config_from_parts(None, "r8_abc", None, None, false).unwrap();
        assert_eq!(cfg.model, DEFAULT_MODEL);
        assert_eq!(cfg.provider, "replicate");
        assert_eq!(cfg.creativity, 0.6);
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
    fn supir_input_uses_its_real_field_names_not_the_generic_img2img_ones() {
        // Real model, checked live against Replicate's own metadata endpoint (2026-07-20, owner
        // report: "чому фото не генерує" after typing shanginn/supir into the model field) —
        // its actual schema has `captions`/`n_prompt`, never `prompt`/`negative_prompt`/
        // `strength`/`guidance_scale`. Sending the generic fields wastes the owner's
        // prompt/creativity settings at best (SUPIR just runs on its own defaults) and risks an
        // outright schema-validation rejection at worst.
        let v = build_input("shanginn/supir", "data:image/png;base64,xxx", "Monster_Wolf_Diffuse.png", 2, 0.5, false);
        assert!(v.get("captions").and_then(|p| p.as_str()).unwrap().contains("creature"));
        assert!(v.get("n_prompt").is_some());
        for absent in ["prompt", "negative_prompt", "strength", "guidance_scale", "scale_factor", "scale"] {
            assert!(v.get(absent).is_none(), "SUPIR input should not carry the unrelated '{absent}' field: {v}");
        }
    }

    #[test]
    fn regenerate_mode_on_a_generic_img2img_model_uses_the_reimagine_prompt_and_moderate_strength() {
        // "stability-ai/sdxl" is the default "✨ Нові текстури" model — a normal *global*
        // (non-tiled) img2img SDXL model, the right tool for "reimagine while keeping the rough
        // shape" (see the build_input doc comment for why clarity-upscaler's tiled architecture
        // is NOT: pushed to extreme creativity it hallucinates each tile almost independently,
        // producing an incoherent collage instead of one coherent new image). Strength is
        // deliberately capped MODERATE, not maximal — a real incident (2026-07-19) showed
        // strength ~0.9 makes the model ignore the input almost entirely, returning a completely
        // unrelated illustration instead of a repaint of the actual source image.
        let v = build_input("stability-ai/sdxl", "data:image/png;base64,xxx", "Monster_Wolf_Diffuse.png", 2, 0.85, true);
        let prompt = v.get("prompt").and_then(|p| p.as_str()).unwrap();
        assert!(prompt.contains("brand-new"), "{prompt}");
        assert!(!prompt.contains("EXACT same colors"), "regenerate mode must not pin exact colors: {prompt}");
        let strength = v.get("strength").and_then(|s| s.as_f64()).unwrap();
        assert!(strength > 0.4 && strength <= 0.7, "regenerate strength should be moderate (repaint, not ignore input), got {strength}");

        let faithful = build_input("stability-ai/sdxl", "data:image/png;base64,xxx", "Monster_Wolf_Diffuse.png", 2, 0.85, false);
        let faithful_strength = faithful.get("strength").and_then(|s| s.as_f64()).unwrap();
        assert!(strength > faithful_strength, "regenerate must diverge more than the faithful mode at the same creativity");
    }

    #[test]
    fn regenerate_mode_on_clarity_stays_short_of_the_incoherent_tile_collage_cliff() {
        // Real incident (2026-07-19): pinning clarity-upscaler's creativity/resemblance to their
        // absolute extremes (0.85+ / 0.1) turned an ogre portrait into a grid of unrelated faces
        // — its tiled architecture stops being coherent past a point. Regenerate mode on clarity
        // pushes further than faithful mode, but keeps resemblance off the floor.
        let v = build_input("philz1337x/clarity-upscaler", "data:image/png;base64,xxx", "Monster_Wolf_Diffuse.png", 2, 0.5, true);
        assert!(v.get("prompt").and_then(|p| p.as_str()).unwrap().contains("brand-new"));
        let resemblance = v.get("resemblance").and_then(|r| r.as_f64()).unwrap();
        assert!(resemblance >= 0.34, "regenerate on clarity must stay off the incoherent-tile floor, got {resemblance}");
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
    fn guess_creature_name_handles_the_real_owner_screenshot_filenames() {
        // Real filenames from the owner's "Франкенштейн" bug report screenshots (2026-07-19).
        assert_eq!(
            guess_creature_name("Ani_Hero_Monster_Oger_Body_Diffuse_S1.png").as_deref(),
            Some("Oger"),
            "name right after Monster"
        );
        assert_eq!(
            guess_creature_name("Ani_Monster_Body_Nautilus_01_Diffuse_S1.png").as_deref(),
            Some("Nautilus"),
            "body-part word before the name — must skip past it and the trailing 01"
        );
        assert_eq!(guess_creature_name("Ani_Monster_Stingrat_Body_Diffuse_S1.png").as_deref(), Some("Stingrat"));
        assert_eq!(guess_creature_name("Ani_Monster_Stingrat_Claws_Diffuse_S1.png").as_deref(), Some("Stingrat"));
        assert_eq!(
            guess_creature_name("Ani_Monster_Stingrat_Eyes_Diffuse_S1.png").as_deref(),
            Some("Stingrat"),
            "must not mistake the trailing S1 for the name"
        );
        assert_eq!(guess_creature_name("Ani_Monster_Wolf_Body_01_Diffuse_S1.png").as_deref(), Some("Wolf"));
        assert_eq!(guess_creature_name("Ani_Monster_Wolf_Claws_01_Diffuse_S1.png").as_deref(), Some("Wolf"));
        assert_eq!(guess_creature_name("compiled/images/Special/ItWpn_Axes_01_Diffuse_01.png"), None, "not a monster texture at all");
    }

    #[test]
    fn texture_subject_names_the_specific_creature_for_monster_textures() {
        let wolf = texture_prompt("compiled/images/Animation/Monster/Ani_Monster_Wolf_Body_01_Diffuse_S1.png");
        assert!(wolf.contains("Wolf-type creature"), "{wolf}");
        let oger = texture_prompt_regenerate("Ani_Hero_Monster_Oger_Body_Diffuse_S1.png");
        assert!(oger.contains("Oger-type creature"), "{oger}");
        // A non-monster texture keeps its category text unchanged (no creature name to fold in).
        let rock = texture_prompt("compiled/images/Nature/Nat_Stone_Rock_01_Diffuse_03.png");
        assert!(!rock.contains("-type creature"), "{rock}");
    }

    #[test]
    fn atlas_guardrails_forbid_inventing_faces_and_merging_regions() {
        // The actual "Франкенштейн" fix: real screenshots showed a blobby, faceless body-parts
        // atlas turned into a wall of invented monster heads/faces/eyes, and a plain eye crop
        // turned into an unrelated abstract pattern — both prompts must now explicitly forbid
        // this, and both `build_input` branches that take a prompt must carry it in the
        // negative_prompt too (see the matching test below).
        for prompt in [
            texture_prompt("Ani_Monster_Wolf_Body_01_Diffuse_S1.png"),
            texture_prompt_regenerate("Ani_Monster_Wolf_Body_01_Diffuse_S1.png"),
        ] {
            assert!(prompt.contains("texture ATLAS") || prompt.contains("ATLAS"), "{prompt}");
            assert!(prompt.to_lowercase().contains("do not invent a face"), "{prompt}");
            assert!(prompt.to_lowercase().contains("not concept art"), "{prompt}");
        }
    }

    #[test]
    fn atlas_negative_terms_reach_every_prompted_build_input_branch_except_full_scene() {
        for (model, regenerate) in [("philz1337x/clarity-upscaler", false), ("philz1337x/clarity-upscaler", true), ("stability-ai/sdxl", false), ("stability-ai/sdxl", true)] {
            let v = build_input(model, "data:image/png;base64,x", "Ani_Monster_Wolf_Body_01_Diffuse_S1.png", 2, 0.6, regenerate);
            let neg = v.get("negative_prompt").and_then(|p| p.as_str()).unwrap();
            assert!(neg.contains("invented face"), "model={model} regenerate={regenerate}: {neg}");
        }
        // Full-scene concept art (splash/menu/GUI) is the deliberate exception — a real
        // illustrated scene legitimately has characters/faces in it.
        let splash = build_input("stability-ai/sdxl", "data:image/png;base64,x", "GUI_LoadingHint_04.png", 2, 0.6, true);
        let neg = splash.get("negative_prompt").and_then(|p| p.as_str()).unwrap();
        assert!(!neg.contains("invented face"), "{neg}");
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
