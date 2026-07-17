import { useEffect, useRef } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";
import { OBJLoader } from "three/examples/jsm/loaders/OBJLoader.js";
import { computeFraming } from "../lib/framing";
import { groupFacesByMaterial } from "../lib/materials";
import { looksDxt5nmSwizzled, reconstructTangentNormalMap } from "../lib/normalMap";
import type { BoneMotion, SkeletonNode, SkinnedMeshData } from "../lib/types";

/** See the matching helper in Model3DViewer.tsx: Genome's normal maps are DXT5-compressed
 * with X/Y swizzled into green/alpha (Z dropped entirely), which three.js has no idea about —
 * left unpacked, every actor with a normal map self-shadowed into a near-black silhouette. */
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

interface Props {
  nodes: SkeletonNode[];
  tracks: BoneMotion[];
  playing: boolean;
  /** Real per-vertex bone weights (`xmesh_skin::parse_skinned_mesh` on the Rust side) — when
   * present, the mesh surface actually deforms with the skeleton via a real `THREE.SkinnedMesh`.
   * `objUrl` (the static bind-pose `.obj` `Model3DViewer` also renders) is only used as a
   * fallback when this isn't available. */
  skinnedMesh?: SkinnedMeshData | null;
  objUrl?: string | null;
  diffuseUrl?: string | null;
  normalUrl?: string | null;
  /** Resolves a texture base name to a loadable URL — used with the skinned mesh's own real
   * material list (`skinnedMesh.materials` + `faceMaterialIds`) so multi-material actors get
   * each part's own texture (Wolf = Body + Claws + engine-default; one texture stretched over
   * all three was visibly wrong vs. Rimy3D). Falls back to `diffuseUrl`/`normalUrl` for
   * materials without their own texture reference. */
  resolveTexture?: ((baseName: string) => Promise<string | null>) | null;
  showSkeleton: boolean;
  /** Manual front/back flip for the exceptions the automatic mapping gets backwards (real,
   * confirmed per-actor data inconsistency — see the doc comment below). Off by default.
   * Split into two independent flags (rather than one "flip everything" toggle) because
   * mirroring the skeleton and the mesh by the SAME amount can never change their relative
   * alignment to each other (it's the same rigid rotation applied to both) — it only changes
   * which way the whole rig faces the camera. These are for the real, separate case: an actor
   * whose raw skin/mesh data and skeleton data turn out to NOT be in the same coordinate
   * convention (the general case this project already hit once for Wolf — see
   * risenlab-animation-research memory — and fixed by making the mesh a passthrough; if that
   * fix doesn't hold for some other actor, these let a human realign them by hand instead of
   * needing a new one-off code fix each time). */
  mirrorSkeleton: boolean;
  mirrorMesh: boolean;
}

/** Brute-forced all 24 proper signed-axis-permutation candidates for position+rotation against
 * the real, already-correctly-oriented bind-pose `.obj` mesh's own bounding box (`Model3DViewer`
 * renders that same mesh with zero coordinate fix-up), scoring how many of a real actor's bones
 * land inside it: for the Wolf, exactly two candidates scored 0-outside — plain identity, and a
 * 180°-yaw mirror of it — indistinguishable by bounding-box fit alone (they're mirror images
 * along the front/back axis). Identity is the confirmed-correct one for the Wolf (owner
 * confirmed live: "тепер голова там де і повинна бути" after testing this exact mapping).
 * **Known real limitation, not yet solved**: at least one other actor (Pig) shows front/back
 * reversed under this same identity mapping, and another (Nautilus, very few bones) shows the
 * skeleton offset from the body entirely — real per-actor convention differences this single
 * global transform can't cover. Don't "fix" this again by guessing a different global constant
 * without re-testing against the specific actor that's wrong; it silently un-fixes whichever
 * actor was already right (this happened once already this session).
 *
 * Rather than keep guessing a global fix, the owner asked for a manual per-actor override
 * instead (`mirrorSkeleton`/`mirrorMesh` props/checkboxes — see the `Props` doc comment for why
 * they're two independent flags, not one) — a 180°-yaw flip (negate X and Z, keep Y; same proper
 * rotation identity applies to the quaternion's imaginary part, `w` untouched) the owner can
 * toggle on for exceptions like Pig without needing a code change each time. */
function toThreePosition([x, y, z]: [number, number, number], mirrored: boolean): THREE.Vector3 {
  return mirrored ? new THREE.Vector3(-x, y, -z) : new THREE.Vector3(x, y, z);
}
function toThreeQuaternion([x, y, z, w]: [number, number, number, number], mirrored: boolean): THREE.Quaternion {
  return (mirrored ? new THREE.Quaternion(-x, y, -z, w) : new THREE.Quaternion(x, y, z, w)).normalize();
}

function lerpVec3Track(keys: [number, number, number, number][], t: number, mirrored: boolean): THREE.Vector3 | null {
  if (keys.length === 0) return null;
  if (keys.length === 1 || t <= keys[0][3]) return toThreePosition([keys[0][0], keys[0][1], keys[0][2]], mirrored);
  const last = keys[keys.length - 1];
  if (t >= last[3]) return toThreePosition([last[0], last[1], last[2]], mirrored);
  for (let i = 0; i < keys.length - 1; i++) {
    const a = keys[i];
    const b = keys[i + 1];
    if (t >= a[3] && t <= b[3]) {
      const f = (t - a[3]) / (b[3] - a[3] || 1);
      const pa = toThreePosition([a[0], a[1], a[2]], mirrored);
      const pb = toThreePosition([b[0], b[1], b[2]], mirrored);
      return pa.lerp(pb, f);
    }
  }
  return toThreePosition([last[0], last[1], last[2]], mirrored);
}

function slerpQuatTrack(keys: [number, number, number, number, number][], t: number, mirrored: boolean): THREE.Quaternion | null {
  if (keys.length === 0) return null;
  if (keys.length === 1 || t <= keys[0][4]) return toThreeQuaternion([keys[0][0], keys[0][1], keys[0][2], keys[0][3]], mirrored);
  const last = keys[keys.length - 1];
  if (t >= last[4]) return toThreeQuaternion([last[0], last[1], last[2], last[3]], mirrored);
  for (let i = 0; i < keys.length - 1; i++) {
    const a = keys[i];
    const b = keys[i + 1];
    if (t >= a[4] && t <= b[4]) {
      const f = (t - a[4]) / (b[4] - a[4] || 1);
      const qa = toThreeQuaternion([a[0], a[1], a[2], a[3]], mirrored);
      const qb = toThreeQuaternion([b[0], b[1], b[2], b[3]], mirrored);
      return qa.slerp(qb, f);
    }
  }
  return toThreeQuaternion([last[0], last[1], last[2], last[3]], mirrored);
}

export function motionDuration(tracks: BoneMotion[]): number {
  let d = 0;
  for (const t of tracks) {
    const lastPos = t.positionKeys[t.positionKeys.length - 1];
    const lastRot = t.rotationKeys[t.rotationKeys.length - 1];
    const lastScale = t.scaleKeys[t.scaleKeys.length - 1];
    if (lastPos) d = Math.max(d, lastPos[3]);
    if (lastRot) d = Math.max(d, lastRot[4]);
    if (lastScale) d = Math.max(d, lastScale[3]);
  }
  return d;
}

export default function SkeletonAnimationViewer({ nodes, tracks, playing, skinnedMesh, objUrl, diffuseUrl, normalUrl, resolveTexture, showSkeleton, mirrorSkeleton, mirrorMesh }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || nodes.length === 0) return;

    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x14161b);
    const camera = new THREE.PerspectiveCamera(45, 1, 0.1, 5000);
    const renderer = new THREE.WebGLRenderer({ antialias: true, logarithmicDepthBuffer: true });
    // Same "шейдери" upgrade as Model3DViewer: ACES filmic tone mapping + procedural IBL —
    // see the matching comment there.
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.1;
    const pmrem = new THREE.PMREMGenerator(renderer);
    scene.environment = pmrem.fromScene(new RoomEnvironment(), 0.04).texture;
    scene.environmentIntensity = 0.55;
    renderer.setPixelRatio(1);
    renderer.domElement.style.display = "block";
    container.appendChild(renderer.domElement);
    const controls = new OrbitControls(camera, renderer.domElement);

    // Even, neutral lighting — see the matching comment in Model3DViewer.tsx: a single key
    // light left the model's far side nearly black, which read as "broken textures".
    scene.add(new THREE.HemisphereLight(0xffffff, 0x445566, 1.5));
    const key = new THREE.DirectionalLight(0xffffff, 1.4);
    key.position.set(3, 5, 4);
    scene.add(key);
    const fill = new THREE.DirectionalLight(0xffffff, 1.0);
    fill.position.set(-4, 2, -3);
    scene.add(fill);

    const bones = nodes.map((n) => {
      const bone = new THREE.Bone();
      bone.position.copy(toThreePosition(n.position, mirrorSkeleton));
      bone.quaternion.copy(toThreeQuaternion(n.rotation, mirrorSkeleton));
      return bone;
    });
    const root = new THREE.Group();
    nodes.forEach((n, i) => {
      if (n.parentIndex !== null && bones[n.parentIndex]) bones[n.parentIndex].add(bones[i]);
      else root.add(bones[i]);
    });
    scene.add(root);
    // SkeletonHelper (and THREE.Skeleton's automatic bind-matrix computation below) both read
    // each bone's `matrixWorld` right here — bones default to an identity matrixWorld until the
    // scene graph is explicitly updated at least once, so without this both would compute wrong
    // (collapsed-to-origin, or wrong bind pose) results.
    root.updateMatrixWorld(true);

    let meshObject: THREE.Object3D | null = null;
    const textureLoader = new THREE.TextureLoader();
    // See the matching comment in Model3DViewer.tsx: a failed load used to be silently
    // invisible (map just stays unset, mesh still renders lit/shaded and can look
    // plausibly "textured" at a glance). Log loudly and paint an unmistakable error color
    // instead of leaving a failed load looking like a real successful one.
    const markTextureFailed = (target: THREE.MeshStandardMaterial, kind: string, failedUrl: string, error: unknown) => {
      console.error(`[SkeletonAnimationViewer] ${kind} texture failed to load: ${failedUrl}`, error);
      target.color.set(0xff00ff);
      target.needsUpdate = true;
    };
    // Real game UVs run outside [0,1] (the engine tiles/wraps by default) — see the matching
    // comment in Model3DViewer.tsx: three.js's clamp-to-edge default smears edge pixels across
    // every out-of-range face. flipY stays false here: this path's UV data comes raw from the
    // ._xmac bytes (never flipped to GL's bottom-left-origin convention), so three.js's
    // default `flipY = true` would double up into a full vertical mirror.
    const configureWrap = (tex: THREE.Texture) => {
      tex.wrapS = THREE.RepeatWrapping;
      tex.wrapT = THREE.RepeatWrapping;
      tex.flipY = false;
    };
    const loadTexturesInto = (target: THREE.MeshStandardMaterial, dUrl: string | null, nUrl: string | null) => {
      if (dUrl) {
        textureLoader.load(
          dUrl,
          (tex) => {
            tex.colorSpace = THREE.SRGBColorSpace;
            configureWrap(tex);
            target.map = tex;
            target.needsUpdate = true;
          },
          undefined,
          (err) => markTextureFailed(target, "diffuse", dUrl, err),
        );
      }
      if (nUrl) {
        textureLoader.load(
          nUrl,
          (tex) => {
            configureWrap(tex);
            unswizzleNormalTexture(tex);
            target.normalMap = tex;
            target.needsUpdate = true;
          },
          undefined,
          (err) => markTextureFailed(target, "normal", nUrl, err),
        );
      }
    };
    const material = new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.75, metalness: 0.1 });
    loadTexturesInto(material, diffuseUrl ?? null, normalUrl ?? null);

    // CPU skinning (not THREE.SkinnedMesh — see risenlab-animation-research memory: the
    // built-in GPU path rendered visibly mangled geometry even at rest pose, and the bug in it
    // was never found; computing final positions ourselves each frame is more code but every
    // step is a plain, inspectable matrix multiply — no bindMatrix/boneInverse bookkeeping
    // hidden inside three.js to get subtly wrong).
    let skinning: {
      geometry: THREE.BufferGeometry;
      bindPositions: Float32Array;
      bindNormals: Float32Array;
      boneIndices: Uint16Array; // 4 per vertex
      boneWeights: Float32Array; // 4 per vertex
      boneBindInverse: THREE.Matrix4[];
    } | null = null;

    if (skinnedMesh && skinnedMesh.positions.length > 0) {
      const vertCount = skinnedMesh.positions.length;
      const bindPositions = new Float32Array(vertCount * 3);
      const bindNormals = new Float32Array(vertCount * 3);
      const uvArr = new Float32Array(vertCount * 2);
      const boneIndices = new Uint16Array(vertCount * 4);
      const boneWeights = new Float32Array(vertCount * 4);
      for (let i = 0; i < vertCount; i++) {
        const p = toThreePosition(skinnedMesh.positions[i], mirrorMesh);
        bindPositions[i * 3] = p.x;
        bindPositions[i * 3 + 1] = p.y;
        bindPositions[i * 3 + 2] = p.z;
        const n = skinnedMesh.normals[i];
        if (n) {
          const nv = toThreePosition(n, mirrorMesh);
          bindNormals[i * 3] = nv.x;
          bindNormals[i * 3 + 1] = nv.y;
          bindNormals[i * 3 + 2] = nv.z;
        }
        const uv = skinnedMesh.uvs[i];
        if (uv) {
          uvArr[i * 2] = uv[0];
          uvArr[i * 2 + 1] = uv[1];
        }
        const weights = skinnedMesh.skinWeights[i] ?? [];
        if (weights.length === 0) {
          // A vertex with no skin data at all is meant to stay rigidly at its bind position —
          // bind fully to bone 0 (a real static root-level node in every actor checked so far)
          // as a rigid approximation, rather than leave all weights at 0 (which would zero out
          // its contribution entirely instead of holding still).
          boneIndices[i * 4] = 0;
          boneWeights[i * 4] = 1;
        } else {
          const total = weights.reduce((s, [, w]) => s + w, 0) || 1;
          for (let k = 0; k < Math.min(4, weights.length); k++) {
            boneIndices[i * 4 + k] = weights[k][0];
            boneWeights[i * 4 + k] = weights[k][1] / total;
          }
        }
      }

      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute("position", new THREE.BufferAttribute(bindPositions.slice(), 3));
      geometry.setAttribute("normal", new THREE.BufferAttribute(bindNormals.slice(), 3));
      geometry.setAttribute("uv", new THREE.BufferAttribute(uvArr, 2));

      // Multi-material rendering from the actor's own real material list: faces regrouped
      // into contiguous per-material runs (`groupFacesByMaterial`), one geometry group + one
      // material per real material, each loading its own referenced diffuse/normal. Materials
      // with no texture reference at all (the engine's "EMFX_Default") are HIDDEN when any
      // textured material exists: on real actors those faces are a skinned collision hull
      // (a whole second low-poly mesh section spanning the entire body — verified on the real
      // Wolf: 500 faces, own vertex range, node "collisionMesh"), and painting it with the
      // fallback texture drew a flat smeared shell OVER the real fur/skin.
      let meshMaterials: THREE.Material | THREE.Material[] = material;
      const realMaterials = skinnedMesh.materials ?? [];
      const faceIds = skinnedMesh.faceMaterialIds ?? [];
      if (resolveTexture && realMaterials.length > 1 && faceIds.length === skinnedMesh.faces.length) {
        const { index, groups } = groupFacesByMaterial(skinnedMesh.faces, faceIds);
        geometry.setIndex(index);
        const anyTextured = realMaterials.some((m) => m?.diffuse || m?.normal);
        const materialList: THREE.MeshStandardMaterial[] = [];
        groups.forEach((group, slot) => {
          geometry.addGroup(group.start, group.count, slot);
          const real = realMaterials[group.materialId];
          const target = new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.75, metalness: 0.1 });
          materialList.push(target);
          if (real?.diffuse || real?.normal) {
            Promise.all([
              real.diffuse ? resolveTexture(real.diffuse) : Promise.resolve(null),
              real.normal ? resolveTexture(real.normal) : Promise.resolve(null),
            ])
              .then(([dUrl, nUrl]) => {
                if (disposed) return;
                if (dUrl || nUrl) loadTexturesInto(target, dUrl, nUrl);
                else loadTexturesInto(target, diffuseUrl ?? null, normalUrl ?? null);
              })
              .catch(() => loadTexturesInto(target, diffuseUrl ?? null, normalUrl ?? null));
          } else if (anyTextured) {
            target.visible = false;
          } else {
            loadTexturesInto(target, diffuseUrl ?? null, normalUrl ?? null);
          }
        });
        meshMaterials = materialList;
      } else {
        geometry.setIndex(skinnedMesh.faces.flat());
      }
      if (bindNormals.every((v) => v === 0)) geometry.computeVertexNormals();

      // Bind-pose inverse world matrix per bone, captured now (right after
      // `root.updateMatrixWorld(true)`, so every bone's matrixWorld reflects its real bind
      // pose) — this is the one piece of state CPU skinning needs to precompute once.
      const boneBindInverse = bones.map((b) => b.matrixWorld.clone().invert());

      const meshMaterial = new THREE.Mesh(geometry, meshMaterials);
      scene.add(meshMaterial);
      meshObject = meshMaterial;
      skinning = { geometry, bindPositions, bindNormals, boneIndices, boneWeights, boneBindInverse };
    } else if (objUrl) {
      new OBJLoader().load(objUrl, (object) => {
        object.traverse((child) => {
          if (child instanceof THREE.Mesh) {
            child.material = material;
            // (-x, y, -z) is a 180°-yaw ROTATION (two axes negated, determinant +1), not a
            // mirror/reflection, so face winding stays correct with no normal/culling fixup
            // needed — same transform `toThreePosition` applies elsewhere.
            if (mirrorMesh) child.geometry.scale(-1, 1, -1);
          }
        });
        // Unlike the skeleton (parsed directly from raw .xmac bytes, needing its own coordinate
        // fix above), this .obj was already written by mimicry-helper's own Max→OBJ conversion
        // — same shape Model3DViewer renders — so it needs no extra rotation here by default.
        // Static fallback only (no skin data available) — doesn't move with the skeleton.
        scene.add(object);
        meshObject = object;
      });
    }

    const helper = new THREE.SkeletonHelper(root);
    (helper.material as THREE.LineBasicMaterial).linewidth = 2;
    helper.visible = showSkeleton;
    scene.add(helper);

    // Frame the camera on the real bind-pose bounds. `Box3.setFromObject` can't be used here —
    // it only measures renderable geometry, and a bare `THREE.Bone` hierarchy has none, so it
    // always comes back empty (collapsing the camera frustum to a near-zero radius and
    // clipping the entire skeleton outside the far plane). Expand the box from each bone's
    // real world position instead.
    const box = new THREE.Box3();
    const worldPos = new THREE.Vector3();
    for (const bone of bones) box.expandByPoint(bone.getWorldPosition(worldPos));
    const size = box.getSize(new THREE.Vector3());
    const center = box.getCenter(new THREE.Vector3());
    const { fitDistance, near, far } = computeFraming(size, camera.fov);
    camera.position.set(center.x + fitDistance * 0.6, center.y + fitDistance * 0.5, center.z + fitDistance);
    camera.near = near;
    camera.far = far;
    camera.updateProjectionMatrix();
    controls.target.copy(center);
    controls.update();
    const radius = Math.max(size.length(), 1);

    scene.add(new THREE.AxesHelper(radius * 0.3));

    let disposed = false;
    let clockStart = performance.now();
    let isPlaying = playing;
    const duration = motionDuration(tracks) || 1;

    function applyPoseAt(t: number) {
      nodes.forEach((_n, i) => {
        const track = tracks[i];
        if (!track) return;
        const pos = lerpVec3Track(track.positionKeys, t, mirrorSkeleton);
        if (pos) bones[i].position.copy(pos);
        const rot = slerpQuatTrack(track.rotationKeys, t, mirrorSkeleton);
        if (rot) bones[i].quaternion.copy(rot);
      });
    }
    // CPU skinning: for each vertex, `newPos = sum_k weight_k * (boneMatrixWorld_k *
    // boneBindInverse_k) * bindPos` — the same weighted-sum-of-bone-deltas formula GPU skinning
    // uses internally, just computed here in plain JS/three.js math instead of a vertex shader.
    const tmpMatrix = new THREE.Matrix4();
    const tmpVec = new THREE.Vector3();
    const tmpNormalVec = new THREE.Vector3();
    function updateSkinning() {
      if (!skinning) return;
      const { geometry, bindPositions, bindNormals, boneIndices, boneWeights, boneBindInverse } = skinning;
      const positionAttr = geometry.attributes.position as THREE.BufferAttribute;
      const normalAttr = geometry.attributes.normal as THREE.BufferAttribute;
      const vertCount = bindPositions.length / 3;
      for (let i = 0; i < vertCount; i++) {
        const bx = bindPositions[i * 3];
        const by = bindPositions[i * 3 + 1];
        const bz = bindPositions[i * 3 + 2];
        const nx = bindNormals[i * 3];
        const ny = bindNormals[i * 3 + 1];
        const nz = bindNormals[i * 3 + 2];
        let outX = 0;
        let outY = 0;
        let outZ = 0;
        let outNX = 0;
        let outNY = 0;
        let outNZ = 0;
        for (let k = 0; k < 4; k++) {
          const w = boneWeights[i * 4 + k];
          if (w === 0) continue;
          const boneIdx = boneIndices[i * 4 + k];
          const bone = bones[boneIdx];
          if (!bone) continue;
          tmpMatrix.multiplyMatrices(bone.matrixWorld, boneBindInverse[boneIdx]);
          tmpVec.set(bx, by, bz).applyMatrix4(tmpMatrix);
          outX += tmpVec.x * w;
          outY += tmpVec.y * w;
          outZ += tmpVec.z * w;
          tmpNormalVec.set(nx, ny, nz).transformDirection(tmpMatrix);
          outNX += tmpNormalVec.x * w;
          outNY += tmpNormalVec.y * w;
          outNZ += tmpNormalVec.z * w;
        }
        positionAttr.setXYZ(i, outX, outY, outZ);
        normalAttr.setXYZ(i, outNX, outNY, outNZ);
      }
      positionAttr.needsUpdate = true;
      normalAttr.needsUpdate = true;
      // Mutating the position attribute in place does NOT update the geometry's bounding
      // sphere — the renderer's frustum-culling check reads that bounding sphere, and a stale
      // or (initially) unset one means the mesh can silently fail to render at all, or use the
      // wrong bounds, regardless of whether the actual vertex data is correct.
      geometry.computeBoundingSphere();
    }

    applyPoseAt(0);
    root.updateMatrixWorld(true);
    updateSkinning();

    let frame = 0;
    function animate() {
      frame = requestAnimationFrame(animate);
      if (isPlaying) {
        const elapsed = ((performance.now() - clockStart) / 1000) % duration;
        applyPoseAt(elapsed);
        root.updateMatrixWorld(true);
        updateSkinning();
      }
      controls.update();
      renderer.render(scene, camera);
    }
    animate();

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
    }
    resize();
    const resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(container);

    const setPlaying = (p: boolean) => {
      if (p && !isPlaying) clockStart = performance.now();
      isPlaying = p;
    };
    setPlaying(playing);
    const setShowSkeleton = (v: boolean) => {
      helper.visible = v;
    };
    // Re-read `playing`/`showSkeleton` on every prop change without tearing the whole scene
    // down (a fresh WebGL context per click would be wasteful and would fight the same
    // context-exhaustion issue documented in Model3DViewer.tsx).
    const el = container as HTMLDivElement & { __setPlaying?: (p: boolean) => void; __setShowSkeleton?: (v: boolean) => void };
    el.__setPlaying = setPlaying;
    el.__setShowSkeleton = setShowSkeleton;

    return () => {
      disposed = true;
      cancelAnimationFrame(frame);
      resizeObserver.disconnect();
      controls.dispose();
      renderer.dispose();
      renderer.forceContextLoss();
      if (meshObject) scene.remove(meshObject);
      container.removeChild(renderer.domElement);
      void disposed;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- nodes/tracks identity change (new
    // motion clip/actor selected) is the real remount signal; `playing`/`showSkeleton` are
    // handled without remount below, and objUrl/diffuseUrl/normalUrl always change together
    // with nodes/tracks (a new actor selection) in this screen's real usage. `mirrorSkeleton`/
    // `mirrorMesh` are rare manual toggles, not worth a live-update path — remounting is
    // simplest and cheap.
  }, [nodes, tracks, mirrorSkeleton, mirrorMesh, resolveTexture]);

  useEffect(() => {
    const container = containerRef.current as (HTMLDivElement & { __setPlaying?: (p: boolean) => void }) | null;
    container?.__setPlaying?.(playing);
  }, [playing]);

  useEffect(() => {
    const container = containerRef.current as (HTMLDivElement & { __setShowSkeleton?: (v: boolean) => void }) | null;
    container?.__setShowSkeleton?.(showSkeleton);
  }, [showSkeleton]);

  return <div ref={containerRef} style={{ width: "100%", height: "100%" }} />;
}
