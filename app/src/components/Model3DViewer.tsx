import { useEffect, useRef } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";
import { OBJLoader } from "three/examples/jsm/loaders/OBJLoader.js";
import { computeFraming } from "../lib/framing";
import { deriveNormalName, deriveSpecularName } from "../lib/materials";
import { looksDxt5nmSwizzled, reconstructTangentNormalMap } from "../lib/normalMap";
import { specularLuminanceToRoughness } from "../lib/roughness";
import type { CameraSyncRef } from "../lib/cameraSync";

/** Genome's normal maps are DXT5-compressed with the X/Y components swizzled into the green
 * and alpha channels (see lib/normalMap.ts) — three.js has no idea about this and reads R/G/B
 * directly as (X,Y,Z), which decoded every real normal map in this game as a near-uniform
 * grazing-angle tilt (confirmed: a "textured" axe rendered as a near-black silhouette; disabling
 * its normal map alone revealed the correct diffuse detail was there all along). Unswizzle via
 * an offscreen canvas before handing the texture to three.js so the lighting response is real. */
function unswizzleNormalTexture(tex: THREE.Texture): void {
  const image = tex.image as HTMLImageElement;
  const canvas = document.createElement("canvas");
  canvas.width = image.naturalWidth || image.width;
  canvas.height = image.naturalHeight || image.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.drawImage(image, 0, 0);
  const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
  if (!looksDxt5nmSwizzled(imageData.data)) return;
  reconstructTangentNormalMap(imageData.data);
  ctx.putImageData(imageData, 0, 0);
  tex.image = canvas;
  tex.needsUpdate = true;
}

/** Real per-material specular-as-roughness (owner audit item, see lib/roughness.ts for the
 * conversion + its accuracy caveat) — replaces the single flat `roughness: 0.75` every
 * material used regardless of what it actually was. */
function applySpecularAsRoughness(tex: THREE.Texture): void {
  const image = tex.image as HTMLImageElement;
  const canvas = document.createElement("canvas");
  canvas.width = image.naturalWidth || image.width;
  canvas.height = image.naturalHeight || image.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.drawImage(image, 0, 0);
  const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
  specularLuminanceToRoughness(imageData.data);
  ctx.putImageData(imageData, 0, 0);
  tex.image = canvas;
  tex.needsUpdate = true;
}

export type ViewMode = "textured" | "wireframe" | "clay" | "normalMap";

interface Props {
  objUrl: string;
  diffuseUrl: string | null;
  normalUrl: string | null;
  mode: ViewMode;
  /** Resolves a texture base name (a material's `usemtl` name — which in this game's real
   * data IS its diffuse texture's base name) to a loadable URL. When present, multi-material
   * meshes get each submesh's own real texture instead of the first material's texture
   * stretched over everything (real bug: the Titan weapons pair two texture atlases; with a
   * single material the sword-misc parts rendered with the axes atlas — "incomplete" textures
   * confirmed against Rimy3D, which reads the full .mtl). Null/absent = single-material
   * behavior (also used to honor an explicit user texture override). */
  resolveTexture?: ((baseName: string) => Promise<string | null>) | null;
  /** Shared with a sibling viewer (owner request, 2026-07-19): rotating/zooming this camera
   * mirrors onto the other panel and vice versa — see lib/cameraSync.ts for why this is a plain
   * ref rather than React state. Absent/null = no sync, normal independent orbit. */
  cameraSync?: CameraSyncRef | null;
}

/** Frames the camera so the whole model fits the view, regardless of its native scale
 * (game meshes come in wildly different unit sizes — a sword vs. a standing stone). The
 * actual math is `computeFraming` (lib/framing.ts), unit-tested without needing a WebGL
 * context; this just applies it to the real three.js objects. */
function frameObject(object: THREE.Object3D, camera: THREE.PerspectiveCamera, controls: OrbitControls) {
  const box = new THREE.Box3().setFromObject(object);
  const size = box.getSize(new THREE.Vector3());
  const center = box.getCenter(new THREE.Vector3());
  const { fitDistance, near, far } = computeFraming(size, camera.fov);

  object.position.sub(center);
  camera.position.set(fitDistance * 0.6, fitDistance * 0.5, fitDistance);
  camera.near = near;
  camera.far = far;
  camera.updateProjectionMatrix();
  controls.target.set(0, 0, 0);
  controls.update();
}

export default function Model3DViewer({ objUrl, diffuseUrl, normalUrl, mode, resolveTexture, cameraSync }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x14161b);

    const camera = new THREE.PerspectiveCamera(45, 1, 0.01, 1000);
    // No antialiasing and a capped pixel ratio: this needs to run acceptably even under a
    // software (non-GPU-accelerated) WebGL fallback, where MSAA is disproportionately
    // expensive per frame.
    // Game meshes routinely layer a second near-coincident surface on top of the base geometry
    // for lens/rune-glow "decal" effects (e.g. a helmet's crystal visor, a sword's glow overlay) —
    // real examples the owner flagged as rendering as stripes/noise instead of a clean surface.
    // The standard WebGL depth buffer doesn't have enough precision to tell those two near-identical
    // depths apart at typical camera distances, so they flicker/interleave (z-fighting). A
    // logarithmic depth buffer gives much finer precision in exactly that near-camera range.
    const renderer = new THREE.WebGLRenderer({ antialias: false, powerPreference: "low-power", logarithmicDepthBuffer: true });
    // --- "Шейдери" (owner request): filmic tone mapping + image-based lighting ---------
    // ACES filmic curve stops the washed-out/flat look of raw lighting, and a procedural
    // RoomEnvironment (generated locally — no external assets) gives MeshStandardMaterial
    // real reflections/speculars instead of dead matte shading.
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.1;
    const pmrem = new THREE.PMREMGenerator(renderer);
    scene.environment = pmrem.fromScene(new RoomEnvironment(), 0.04).texture;
    scene.environmentIntensity = 0.55;
    renderer.setPixelRatio(1);
    // `display:block` avoids the few extra inline-baseline pixels a bare <canvas> reserves,
    // which — combined with a container sized from its own content — can make the
    // ResizeObserver below and this element's own size chase each other every frame.
    renderer.domElement.style.display = "block";
    container.appendChild(renderer.domElement);

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = false;

    // Even, neutral lighting: this viewer's job is judging textures (like Rimy3D's flat-lit
    // view), not dramatic presentation. The previous single key + BLUE rim left the far side
    // of every model nearly black with a navy tint — the owner read that as "broken/black
    // textures" when the texture data was actually fine.
    scene.add(new THREE.HemisphereLight(0xffffff, 0x445566, 1.5));
    const key = new THREE.DirectionalLight(0xffffff, 1.4);
    key.position.set(3, 5, 4);
    scene.add(key);
    const fill = new THREE.DirectionalLight(0xffffff, 1.0);
    fill.position.set(-4, 2, -3);
    scene.add(fill);

    let disposed = false;
    let currentObject: THREE.Object3D | null = null;
    const textureLoader = new THREE.TextureLoader();

    function newMaterial(): THREE.MeshStandardMaterial {
      // Four honest, distinct render modes (no fabricated data): the real diffuse+normal
      // shading, a wireframe of the real topology, an untextured "clay" pass for judging
      // silhouette/shape, and a direct view of the normal/relief map itself (the "green
      // file" the owner asked about) as the surface color, to inspect it on its own.
      return new THREE.MeshStandardMaterial({
        color: mode === "clay" ? 0x9099a6 : 0xffffff,
        wireframe: mode === "wireframe",
        roughness: 0.75,
        metalness: 0.1,
      });
    }

    // A failed texture load used to be silently invisible: `material.map` just stayed
    // unset and the mesh rendered as a plain lit metallic-ish shape — real (from a normal
    // map applying while diffuse failed) shading detail made it look plausibly "textured"
    // at a glance even though it wasn't (confirmed: owner caught this live, a "detailed"
    // looking axe had zero real network requests for its texture — a stale/failed load
    // masquerading as success). Every load now has a real onError that (a) logs loudly with
    // the failing URL so this is diagnosable instead of guessed at, and (b) paints the
    // material an unmistakable error color instead of leaving it looking like a real,
    // successfully-shaded metal/clay surface.
    function markTextureFailed(material: THREE.MeshStandardMaterial, kind: string, failedUrl: string, error: unknown) {
      console.error(`[Model3DViewer] ${kind} texture failed to load: ${failedUrl}`, error);
      material.color.set(0xff00ff);
      material.needsUpdate = true;
      requestRender();
    }

    // Real game UVs routinely run outside [0,1] (e.g. It_Wpn_Axe_Titan: every one of its 1285
    // UVs has a NEGATIVE V — the D3D-era engine tiles/wraps textures by default). three.js's
    // TextureLoader defaults to ClampToEdgeWrapping, which smears the texture's edge pixels
    // across every out-of-range face — that rendered whole weapon blades as flat grey even
    // though the UVs and the texture were both correct.
    // flipY stays true: these meshes come from a real, verified-correct .obj (Risenaut's own
    // xmsh->obj export, opened correctly in Rimy3D by the owner), whose UVs follow the
    // standard OBJ bottom-left-origin convention (confirmed live A/B against Rimy3D's render
    // of the same real .obj/.png — the apple).
    function configureWrap(tex: THREE.Texture) {
      tex.wrapS = THREE.RepeatWrapping;
      tex.wrapT = THREE.RepeatWrapping;
      tex.flipY = true;
    }

    function loadTexturesInto(material: THREE.MeshStandardMaterial, dUrl: string | null, nUrl: string | null, sUrl?: string | null) {
      if (mode === "textured" && sUrl) {
        textureLoader.load(
          sUrl,
          (tex) => {
            applySpecularAsRoughness(tex);
            material.roughnessMap = tex;
            // Let the map fully drive per-pixel roughness instead of also scaling it down —
            // three.js multiplies the map sample by this scalar.
            material.roughness = 1.0;
            material.needsUpdate = true;
            requestRender();
          },
          undefined,
          (err) => markTextureFailed(material, "specular", sUrl, err),
        );
      }
      if (mode === "textured" && dUrl) {
        textureLoader.load(
          dUrl,
          (tex) => {
            tex.colorSpace = THREE.SRGBColorSpace;
            configureWrap(tex);
            material.map = tex;
            material.needsUpdate = true;
            requestRender();
          },
          undefined,
          (err) => markTextureFailed(material, "diffuse", dUrl, err),
        );
      }
      if (mode === "textured" && nUrl) {
        textureLoader.load(
          nUrl,
          (tex) => {
            configureWrap(tex);
            unswizzleNormalTexture(tex);
            material.normalMap = tex;
            material.needsUpdate = true;
            requestRender();
          },
          undefined,
          (err) => markTextureFailed(material, "normal", nUrl, err),
        );
      }
      if (mode === "normalMap" && nUrl) {
        textureLoader.load(
          nUrl,
          (tex) => {
            configureWrap(tex);
            material.map = tex;
            material.needsUpdate = true;
            requestRender();
          },
          undefined,
          (err) => markTextureFailed(material, "normalMap-preview", nUrl, err),
        );
      }
    }

    function applyMaterial(object: THREE.Object3D) {
      const fallback = newMaterial();
      loadTexturesInto(fallback, diffuseUrl, normalUrl);
      // Each `usemtl` name is this game's real per-submesh texture key (see the
      // `resolveTexture` prop doc). One shared material per distinct name, resolved once;
      // anything unresolvable keeps the fallback (the side panel's auto-matched pair).
      //
      // OBJLoader shape gotcha (a real bug on the Titan axe): with a single `usemtl` per
      // `o`/`g` group it emits one child mesh per material, but with SEVERAL `usemtl` under
      // one `o` it emits ONE mesh whose `material` is an ARRAY (plus geometry groups). The
      // old code only read `material.name` off single materials and sent every array-material
      // mesh whole to the fallback — so It_Wpn_Axe_Titan's handle (minority material,
      // SwordMisc atlas) was sampled from the blades' Axes atlas: pale stone instead of dark
      // wood/leather. Resolve each array slot by its own name instead.
      const byName = new Map<string, THREE.MeshStandardMaterial>();
      const materialFor = (source: THREE.Material | null | undefined): THREE.MeshStandardMaterial => {
        const materialName = source?.name ?? "";
        if (mode !== "textured" || !resolveTexture || !materialName) return fallback;
        let material = byName.get(materialName);
        if (!material) {
          material = newMaterial();
          byName.set(materialName, material);
          const target = material;
          const normalName = deriveNormalName(materialName);
          const specularName = deriveSpecularName(materialName);
          Promise.all([
            resolveTexture(materialName),
            normalName ? resolveTexture(normalName) : Promise.resolve(null),
            specularName ? resolveTexture(specularName) : Promise.resolve(null),
          ])
            .then(([dUrl, nUrl, sUrl]) => {
              if (disposed) return;
              if (dUrl || nUrl) loadTexturesInto(target, dUrl, nUrl ?? normalUrl, sUrl);
              else loadTexturesInto(target, diffuseUrl, normalUrl, sUrl);
            })
            .catch(() => loadTexturesInto(target, diffuseUrl, normalUrl));
        }
        return material;
      };
      object.traverse((child) => {
        if (!(child instanceof THREE.Mesh)) return;
        // Collision hulls (e.g. actor OBJ exports contain a second `o CollisionMesh` object)
        // have faces with NO texture coordinates at all (`f v//vn`) — OBJLoader then builds
        // their geometry without a `uv` attribute. Drawing such a hull in a texture-sampling
        // mode smears one texel across a full-body shell rendered OVER the real model (the
        // real SwampMummy preview bug). Wireframe/clay modes don't sample textures, so the
        // hull stays visible there (it's genuinely useful to inspect).
        if ((mode === "textured" || mode === "normalMap") && !child.geometry.getAttribute("uv")) {
          child.visible = false;
          return;
        }
        if (Array.isArray(child.material)) {
          child.material = child.material.map((m) => materialFor(m));
        } else {
          child.material = materialFor(child.material);
        }
      });
    }

    // Render-on-demand rather than a continuous 60fps loop: this needs to stay usable even
    // under a slow/software WebGL fallback (no GPU acceleration), where redrawing every
    // frame regardless of whether anything changed is the single biggest avoidable cost.
    let needsRender = true;
    function requestRender() {
      needsRender = true;
    }
    controls.addEventListener("change", requestRender);

    // Camera sync with a sibling side-by-side viewer (see lib/cameraSync.ts). `applyingSync`
    // guards against an echo: `controls.update()` below (called after we set the camera/target
    // programmatically) fires its own "change" event, which would otherwise immediately write
    // this exact same state right back to the ref.
    let applyingSync = false;
    let lastAppliedSyncRev = -1;
    if (cameraSync) {
      controls.addEventListener("change", () => {
        if (applyingSync) return;
        const rev = performance.now();
        lastAppliedSyncRev = rev;
        cameraSync.current = {
          rev,
          position: [camera.position.x, camera.position.y, camera.position.z],
          target: [controls.target.x, controls.target.y, controls.target.z],
        };
      });
    }
    // A render request that lands while the tab/window is backgrounded, occluded, or hasn't
    // finished gaining focus yet can be silently dropped (the browser throttles/skips paints
    // for hidden or not-yet-visible content) — the flag gets set, but no frame ever consumes
    // it, so the canvas is stuck showing nothing until *something else* asks for a render.
    // Re-requesting whenever the page becomes visible/focused again closes that gap.
    document.addEventListener("visibilitychange", requestRender);
    window.addEventListener("focus", requestRender);
    // Also cover the same gap right after mount itself (a fresh mesh/actor selection creates a
    // brand-new canvas every time — see the `key={...}` on this component's call sites — and
    // that mount can itself race with the window still gaining focus/visibility): keep asking
    // for a few more frames over the first second so a dropped initial paint gets a second,
    // third, fourth... chance instead of only one.
    const insuranceRenders = [100, 300, 600, 1000].map((ms) => setTimeout(requestRender, ms));

    new OBJLoader().load(objUrl, (object) => {
      if (disposed) return;
      applyMaterial(object);
      scene.add(object);
      currentObject = object;
      frameObject(object, camera, controls);
      requestRender();
    });

    // Guard against a ResizeObserver feedback loop: only touch the renderer/camera when the
    // container's size actually changed. Without this, setting the canvas size inside the
    // observer callback can re-trigger the same observer indefinitely in some layouts.
    let lastWidth = -1;
    let lastHeight = -1;
    function resize() {
      if (!container) return;
      const { clientWidth, clientHeight } = container;
      if (clientWidth === lastWidth && clientHeight === lastHeight) return;
      lastWidth = clientWidth;
      lastHeight = clientHeight;
      camera.aspect = clientWidth / Math.max(clientHeight, 1);
      camera.updateProjectionMatrix();
      renderer.setSize(clientWidth, clientHeight);
      requestRender();
    }
    resize();
    const resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(container);

    let frame = 0;
    function animate() {
      frame = requestAnimationFrame(animate);
      const syncing = !!(cameraSync?.current && cameraSync.current.rev > lastAppliedSyncRev);
      if (syncing && cameraSync?.current) {
        lastAppliedSyncRev = cameraSync.current.rev;
        applyingSync = true;
        camera.position.set(...cameraSync.current.position);
        controls.target.set(...cameraSync.current.target);
        controls.update();
        applyingSync = false;
        needsRender = true;
      }
      if (!needsRender) return;
      needsRender = false;
      renderer.render(scene, camera);
    }
    animate();

    return () => {
      disposed = true;
      cancelAnimationFrame(frame);
      insuranceRenders.forEach(clearTimeout);
      resizeObserver.disconnect();
      controls.removeEventListener("change", requestRender);
      document.removeEventListener("visibilitychange", requestRender);
      window.removeEventListener("focus", requestRender);
      controls.dispose();
      renderer.dispose();
      // `dispose()` frees three.js-side resources but does NOT deterministically release the
      // underlying WebGL context — that's left to browser GC, which can lag behind. Every
      // mesh/actor selection mounts a brand-new canvas (see the `key={...}` at each call site),
      // so clicking through several models quickly can create new contexts faster than old ones
      // get reclaimed, and browsers cap how many can be alive at once (commonly ~16) — past
      // that cap, a fresh canvas silently never gets a real context and just stays blank
      // forever. Forcing context loss here makes the release immediate and deterministic.
      renderer.forceContextLoss();
      if (currentObject) scene.remove(currentObject);
      container.removeChild(renderer.domElement);
    };
  }, [objUrl, diffuseUrl, normalUrl, mode, resolveTexture]);

  return <div ref={containerRef} style={{ width: "100%", height: "100%" }} />;
}
