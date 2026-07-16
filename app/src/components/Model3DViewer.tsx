import { useEffect, useRef } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { OBJLoader } from "three/examples/jsm/loaders/OBJLoader.js";
import { computeFraming } from "../lib/framing";

export type ViewMode = "textured" | "wireframe" | "clay" | "normalMap";

interface Props {
  objUrl: string;
  diffuseUrl: string | null;
  normalUrl: string | null;
  mode: ViewMode;
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

export default function Model3DViewer({ objUrl, diffuseUrl, normalUrl, mode }: Props) {
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
    renderer.setPixelRatio(1);
    // `display:block` avoids the few extra inline-baseline pixels a bare <canvas> reserves,
    // which — combined with a container sized from its own content — can make the
    // ResizeObserver below and this element's own size chase each other every frame.
    renderer.domElement.style.display = "block";
    container.appendChild(renderer.domElement);

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = false;

    scene.add(new THREE.HemisphereLight(0xffffff, 0x223344, 1.1));
    const key = new THREE.DirectionalLight(0xffffff, 1.6);
    key.position.set(3, 5, 4);
    scene.add(key);
    const rim = new THREE.DirectionalLight(0x6699ff, 0.6);
    rim.position.set(-4, 2, -3);
    scene.add(rim);

    let disposed = false;
    let currentObject: THREE.Object3D | null = null;
    const textureLoader = new THREE.TextureLoader();

    function applyMaterial(object: THREE.Object3D) {
      // Four honest, distinct render modes (no fabricated data): the real diffuse+normal
      // shading, a wireframe of the real topology, an untextured "clay" pass for judging
      // silhouette/shape, and a direct view of the normal/relief map itself (the "green
      // file" the owner asked about) as the surface color, to inspect it on its own.
      const material = new THREE.MeshStandardMaterial({
        color: mode === "clay" ? 0x9099a6 : 0xffffff,
        wireframe: mode === "wireframe",
        roughness: 0.75,
        metalness: 0.1,
      });
      if (mode === "textured" && diffuseUrl) {
        textureLoader.load(diffuseUrl, (tex) => {
          tex.colorSpace = THREE.SRGBColorSpace;
          // Real UV data comes straight from the game's own mesh bytes (Piranha Bytes'
          // Genome engine, a D3D-era engine using a top-left-origin V convention) via
          // mimicry-helper's OBJ writer — it is not re-flipped to match OpenGL/WebGL's
          // bottom-left convention on the way out. three.js's TextureLoader defaults
          // `flipY = true` to match that GL convention, which — combined with UVs that were
          // never flipped to begin with — doubles up into a full vertical mirror of every
          // texture on every model (confirmed live: a texture atlas region meant for the hips
          // ended up rendering on the head). Disable it so the real, unflipped UVs are used
          // as-is, matching what the game engine itself actually samples.
          tex.flipY = false;
          material.map = tex;
          material.needsUpdate = true;
          requestRender();
        });
      }
      if (mode === "textured" && normalUrl) {
        textureLoader.load(normalUrl, (tex) => {
          tex.flipY = false;
          material.normalMap = tex;
          material.needsUpdate = true;
          requestRender();
        });
      }
      if (mode === "normalMap" && normalUrl) {
        textureLoader.load(normalUrl, (tex) => {
          tex.flipY = false;
          material.map = tex;
          material.needsUpdate = true;
          requestRender();
        });
      }
      object.traverse((child) => {
        if (child instanceof THREE.Mesh) {
          child.material = material;
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
  }, [objUrl, diffuseUrl, normalUrl, mode]);

  return <div ref={containerRef} style={{ width: "100%", height: "100%" }} />;
}
