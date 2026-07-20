import { useEffect, useMemo, useRef, useState } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";
import { OBJLoader } from "three/examples/jsm/loaders/OBJLoader.js";
import type { Lang } from "../lib/i18n";
import type { ActorEntry, LibraryEntry, MeshEntry } from "../lib/types";
import { actorObjUrl, listActors, listLibrary, listMeshes, meshObjUrl } from "../lib/api";
import { deriveNormalName, deriveSpecularName, findTextureEntryForBaseName } from "../lib/materials";
import { looksDxt5nmSwizzled, reconstructTangentNormalMap } from "../lib/normalMap";
import { specularLuminanceToRoughness } from "../lib/roughness";
import { categorizeActor, categorizeMesh, type ActorZoneId, type ItemZoneId } from "../lib/showroomCategorize";
import { gridPositions, gridRowCount, normalizeScale, rowPositions, type Vec3 } from "../lib/showroomLayout";

interface Props {
  lang: Lang;
}

/** One thing to load and place — a real mesh or actor, already assigned a world-space slot by
 * the room layout below. `kind` picks `meshObjUrl` vs `actorObjUrl` (actors carry a skeleton the
 * static OBJ export ignores — fine here, this hall shows bind pose only, no animation, per the
 * owner's own scope call: "Від анімацій поки відмовся"). */
interface PlacedEntry {
  key: string;
  kind: "mesh" | "actor";
  entry: MeshEntry | ActorEntry;
  position: Vec3;
  targetSize: number;
}

const ROOM_GAP = 10;
const FIGURE_SPACING = 2.4;

/** The whole hall's layout, computed once real data is in — hand-placed room composition (which
 * real primitive from lib/showroomLayout.ts goes where) built on top of the tested grid/row math,
 * not itself unit-tested (it's presentation, not logic with a right/wrong answer). Mirrors the
 * owner's own request: swords/shields mounted on walls, a table for the rest of the weapon rack,
 * humans/monsters/mobs standing in their own rows, then food/valuables/tools tables — laid out
 * as a straight hall you walk down, room after room, "like Skyrim's qasmoke test cell."
 */
function buildHall(
  itemsByZone: Record<ItemZoneId, MeshEntry[]>,
  actorsByZone: Record<ActorZoneId, ActorEntry[]>,
): { placed: PlacedEntry[]; totalDepth: number; roomStarts: { id: string; label: { uk: string; en: string }; z: number }[] } {
  const placed: PlacedEntry[] = [];
  const roomStarts: { id: string; label: { uk: string; en: string }; z: number }[] = [];

  // --- Room 1: the weapon hall — swords + shields on the two side walls, everything else
  // (axes/staffs/helmets/ammo) laid out on a long table down the middle. ---
  const room1Z = 0;
  roomStarts.push({ id: "weapons", label: { uk: "⚔ Зброя", en: "⚔ Weapons" }, z: room1Z });
  const swords = itemsByZone.swords;
  const swordCols = 7;
  gridPositions({ count: swords.length, columns: swordCols, cellSize: 1.5, origin: [-11, 5, room1Z + 3], axis: "wall" }).forEach((p, i) =>
    placed.push({ key: `mesh:${swords[i].entryPath}`, kind: "mesh", entry: swords[i], position: p, targetSize: 1.3 }),
  );
  const shields = itemsByZone.shields;
  const shieldCols = 4;
  gridPositions({ count: shields.length, columns: shieldCols, cellSize: 1.8, origin: [11, 5, room1Z + 3], axis: "wall" }).forEach((p, i) =>
    placed.push({ key: `mesh:${shields[i].entryPath}`, kind: "mesh", entry: shields[i], position: p, targetSize: 1.6 }),
  );
  const weaponsMisc = itemsByZone.weaponsMisc;
  const miscCols = 8;
  const miscRows = gridRowCount(weaponsMisc.length, miscCols);
  gridPositions({ count: weaponsMisc.length, columns: miscCols, cellSize: 1.6, origin: [-((miscCols - 1) * 1.6) / 2, 1.1, room1Z + 4], axis: "floor" }).forEach((p, i) =>
    placed.push({ key: `mesh:${weaponsMisc[i].entryPath}`, kind: "mesh", entry: weaponsMisc[i], position: p, targetSize: 1.1 }),
  );
  const room1Depth = Math.max(16, miscRows * 1.6 + 8);

  // --- Room 2: the hall of figures — humans, monsters, and mobs each get their own row, all
  // three walking the same length of hall side by side. ---
  const room2Z = room1Z + room1Depth + ROOM_GAP;
  roomStarts.push({ id: "figures", label: { uk: "🧍 Персонажі", en: "🧍 Figures" }, z: room2Z });
  const humans = actorsByZone.humans;
  const monsters = actorsByZone.monsters;
  const mobs = actorsByZone.mobs;
  const maxFigures = Math.max(humans.length, monsters.length, mobs.length, 1);
  const room2Depth = maxFigures * FIGURE_SPACING + 6;
  const rowCenterZ = room2Z + room2Depth / 2;
  rowPositions({ count: humans.length, spacing: FIGURE_SPACING, origin: [-7, 0, rowCenterZ], axis: "z" }).forEach((p, i) =>
    placed.push({ key: `actor:${humans[i].entryPath}`, kind: "actor", entry: humans[i], position: p, targetSize: 2 }),
  );
  rowPositions({ count: monsters.length, spacing: FIGURE_SPACING, origin: [0, 0, rowCenterZ], axis: "z" }).forEach((p, i) =>
    placed.push({ key: `actor:${monsters[i].entryPath}`, kind: "actor", entry: monsters[i], position: p, targetSize: 2.2 }),
  );
  rowPositions({ count: mobs.length, spacing: FIGURE_SPACING, origin: [7, 0, rowCenterZ], axis: "z" }).forEach((p, i) =>
    placed.push({ key: `actor:${mobs[i].entryPath}`, kind: "actor", entry: mobs[i], position: p, targetSize: 1.8 }),
  );

  // --- Room 3: provisions & curios — three tables side by side: food, valuables, tools/books. ---
  const room3Z = room2Z + room2Depth + ROOM_GAP;
  roomStarts.push({ id: "provisions", label: { uk: "🍞 Припаси й скарби", en: "🍞 Provisions & curios" }, z: room3Z });
  const food = itemsByZone.food;
  const valuables = itemsByZone.valuables;
  const tools = itemsByZone.tools;
  const foodCols = 10;
  const valuableCols = 8;
  const toolCols = 10;
  const room3Depth =
    Math.max(gridRowCount(food.length, foodCols), gridRowCount(valuables.length, valuableCols), gridRowCount(tools.length, toolCols)) * 1.1 + 6;
  gridPositions({ count: food.length, columns: foodCols, cellSize: 1.0, origin: [-16, 1, room3Z + 3], axis: "floor" }).forEach((p, i) =>
    placed.push({ key: `mesh:${food[i].entryPath}`, kind: "mesh", entry: food[i], position: p, targetSize: 0.6 }),
  );
  gridPositions({ count: valuables.length, columns: valuableCols, cellSize: 0.9, origin: [-3, 1, room3Z + 3], axis: "floor" }).forEach((p, i) =>
    placed.push({ key: `mesh:${valuables[i].entryPath}`, kind: "mesh", entry: valuables[i], position: p, targetSize: 0.4 }),
  );
  gridPositions({ count: tools.length, columns: toolCols, cellSize: 1.0, origin: [8, 1, room3Z + 3], axis: "floor" }).forEach((p, i) =>
    placed.push({ key: `mesh:${tools[i].entryPath}`, kind: "mesh", entry: tools[i], position: p, targetSize: 0.7 }),
  );

  return { placed, totalDepth: room3Z + room3Depth, roomStarts };
}

const CONCURRENT_LOADS = 5;

export default function Showroom({ lang }: Props) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [meshes, setMeshes] = useState<MeshEntry[] | null>(null);
  const [actors, setActors] = useState<ActorEntry[] | null>(null);
  const [textures, setTextures] = useState<LibraryEntry[]>([]);
  const [loaded, setLoaded] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const jumpToRoomRef = useRef<((z: number) => void) | null>(null);

  useEffect(() => {
    listMeshes()
      .then(setMeshes)
      .catch((e) => setError(String(e)));
    listActors()
      .then(setActors)
      .catch((e) => setError(String(e)));
    listLibrary()
      .then(setTextures)
      .catch(() => {});
  }, []);

  const hall = useMemo(() => {
    if (!meshes || !actors) return null;
    const itemsByZone: Record<ItemZoneId, MeshEntry[]> = { swords: [], shields: [], weaponsMisc: [], food: [], valuables: [], tools: [] };
    for (const m of meshes) {
      const zone = categorizeMesh(m);
      if (zone) itemsByZone[zone].push(m);
    }
    const actorsByZone: Record<ActorZoneId, ActorEntry[]> = { humans: [], monsters: [], mobs: [] };
    for (const a of actors) {
      const zone = categorizeActor(a);
      if (zone) actorsByZone[zone].push(a);
    }
    return buildHall(itemsByZone, actorsByZone);
  }, [meshes, actors]);

  // Resolves a material's own name to a real texture URL — the SAME per-material auto-match
  // every other viewer in this app uses (see Models.tsx's `resolveTexture`), shared across every
  // object in the hall so there's one texture-library lookup, not one per item.
  const resolveTextureRef = useRef<(baseName: string) => Promise<string | null>>(async () => null);
  useEffect(() => {
    resolveTextureRef.current = async (baseName: string) => {
      const entry = findTextureEntryForBaseName(textures, baseName);
      if (!entry) return null;
      const { readTextureDataUrl } = await import("../lib/api");
      return readTextureDataUrl(entry.pngRel);
    };
  }, [textures]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || !hall) return;
    const currentHall = hall; // narrowed non-null here, but not across the closures below
    let disposed = false;

    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x0d0f14);
    scene.fog = new THREE.Fog(0x0d0f14, 30, 90);

    const camera = new THREE.PerspectiveCamera(55, 1, 0.05, 500);
    camera.position.set(0, 3, -6);
    camera.lookAt(0, 2, 10);

    const renderer = new THREE.WebGLRenderer({ antialias: true, logarithmicDepthBuffer: true });
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.05;
    const pmrem = new THREE.PMREMGenerator(renderer);
    scene.environment = pmrem.fromScene(new RoomEnvironment(), 0.04).texture;
    scene.environmentIntensity = 0.5;
    renderer.setPixelRatio(1);
    renderer.domElement.style.display = "block";
    container.appendChild(renderer.domElement);

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.target.set(0, 2, 6);
    controls.maxDistance = 120;
    controls.update();

    jumpToRoomRef.current = (z: number) => {
      camera.position.set(0, 4, z - 6);
      controls.target.set(0, 2, z + 6);
      controls.update();
    };

    scene.add(new THREE.HemisphereLight(0xffffff, 0x30343d, 1.3));
    const key = new THREE.DirectionalLight(0xfff3e0, 1.1);
    key.position.set(6, 12, 4);
    scene.add(key);
    const fill = new THREE.DirectionalLight(0xdce8ff, 0.6);
    fill.position.set(-6, 6, -4);
    scene.add(fill);

    // Floor spanning the whole hall + side walls for room 1 (the weapon room's swords/shields
    // need a real surface behind them to read as "mounted", not floating in space) — plain
    // procedural geometry, not real game architecture (see the module doc comment for why).
    const floorMat = new THREE.MeshStandardMaterial({ color: 0x1c1f26, roughness: 0.9 });
    const floor = new THREE.Mesh(new THREE.PlaneGeometry(60, hall.totalDepth + 20), floorMat);
    floor.rotation.x = -Math.PI / 2;
    floor.position.set(0, 0, hall.totalDepth / 2);
    scene.add(floor);

    const wallMat = new THREE.MeshStandardMaterial({ color: 0x24242c, roughness: 0.95 });
    const wallGeo = new THREE.BoxGeometry(0.6, 12, 20);
    const leftWall = new THREE.Mesh(wallGeo, wallMat);
    leftWall.position.set(-13, 6, 6);
    scene.add(leftWall);
    const rightWall = leftWall.clone();
    rightWall.position.set(13, 6, 6);
    scene.add(rightWall);

    const tableMat = new THREE.MeshStandardMaterial({ color: 0x3a2c20, roughness: 0.8 });
    const table1 = new THREE.Mesh(new THREE.BoxGeometry(15, 1, 6), tableMat);
    table1.position.set(0, 0.55, 8);
    scene.add(table1);
    const room3Start = hall.roomStarts.find((r) => r.id === "provisions")?.z ?? hall.totalDepth - 20;
    for (const x of [-16, -3, 8]) {
      const t = new THREE.Mesh(new THREE.BoxGeometry(11, 1, 8), tableMat.clone());
      t.position.set(x, 0.55, room3Start + 6);
      scene.add(t);
    }

    // --- Progressive, concurrency-limited loading of every placed item — reuses the exact same
    // material/texture/normal-map-unswizzle logic as Model3DViewer.tsx (kept local here rather
    // than shared: this needs to run per-item across potentially hundreds of objects sharing one
    // scene/lighting setup, a different shape than that component's single-object focus). ---
    const textureLoader = new THREE.TextureLoader();
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
    function configureWrap(tex: THREE.Texture) {
      tex.wrapS = THREE.RepeatWrapping;
      tex.wrapT = THREE.RepeatWrapping;
      tex.flipY = true;
    }
    function loadTexturesInto(material: THREE.MeshStandardMaterial, dUrl: string | null, nUrl: string | null, sUrl: string | null) {
      if (sUrl) {
        textureLoader.load(sUrl, (tex) => {
          applySpecularAsRoughness(tex);
          material.roughnessMap = tex;
          material.roughness = 1.0;
          material.needsUpdate = true;
        });
      }
      if (dUrl) {
        textureLoader.load(dUrl, (tex) => {
          tex.colorSpace = THREE.SRGBColorSpace;
          configureWrap(tex);
          material.map = tex;
          material.needsUpdate = true;
        });
      }
      if (nUrl) {
        textureLoader.load(nUrl, (tex) => {
          configureWrap(tex);
          unswizzleNormalTexture(tex);
          material.normalMap = tex;
          material.needsUpdate = true;
        });
      }
    }
    function applyMaterials(object: THREE.Object3D) {
      const fallback = new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.75, metalness: 0.1 });
      const byName = new Map<string, THREE.MeshStandardMaterial>();
      const materialFor = (source: THREE.Material | null | undefined): THREE.MeshStandardMaterial => {
        const materialName = source?.name ?? "";
        if (!materialName) return fallback;
        let material = byName.get(materialName);
        if (!material) {
          material = new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.75, metalness: 0.1 });
          byName.set(materialName, material);
          const target = material;
          const normalName = deriveNormalName(materialName);
          const specularName = deriveSpecularName(materialName);
          Promise.all([resolveTextureRef.current(materialName), normalName ? resolveTextureRef.current(normalName) : Promise.resolve(null), specularName ? resolveTextureRef.current(specularName) : Promise.resolve(null)])
            .then(([dUrl, nUrl, sUrl]) => {
              if (disposed) return;
              loadTexturesInto(target, dUrl, nUrl, sUrl);
            })
            .catch(() => {});
        }
        return material;
      };
      object.traverse((child) => {
        if (!(child instanceof THREE.Mesh)) return;
        if (!child.geometry.getAttribute("uv")) {
          child.visible = false; // collision hulls — see Model3DViewer.tsx's matching comment
          return;
        }
        child.material = Array.isArray(child.material) ? child.material.map((m) => materialFor(m)) : materialFor(child.material);
      });
    }

    async function loadOne(item: PlacedEntry) {
      const objUrl = item.kind === "mesh" ? await meshObjUrl(item.entry.archivePath, item.entry.entryPath) : await actorObjUrl(item.entry.archivePath, item.entry.entryPath);
      if (disposed) return;
      const object = await new Promise<THREE.Group>((resolve, reject) => new OBJLoader().load(objUrl, resolve, undefined, reject));
      if (disposed) return;
      applyMaterials(object);
      const box = new THREE.Box3().setFromObject(object);
      const size = box.getSize(new THREE.Vector3());
      const center = box.getCenter(new THREE.Vector3());
      object.position.sub(center); // recenter on its own origin before placing
      const scale = normalizeScale([size.x, size.y, size.z], item.targetSize);
      const holder = new THREE.Group();
      holder.add(object);
      holder.scale.setScalar(scale);
      holder.position.set(...item.position);
      scene.add(holder);
    }

    let cancelled = false;
    async function loadAll() {
      const queue = [...currentHall.placed];
      let doneCount = 0;
      async function worker() {
        while (queue.length > 0 && !cancelled) {
          const item = queue.shift();
          if (!item) break;
          try {
            await loadOne(item);
          } catch (e) {
            console.error(`[Showroom] failed to load ${item.entry.name}:`, e);
          }
          doneCount++;
          if (!cancelled) setLoaded(doneCount);
        }
      }
      await Promise.all(Array.from({ length: CONCURRENT_LOADS }, worker));
    }
    void loadAll();

    let frame = 0;
    function animate() {
      frame = requestAnimationFrame(animate);
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

    return () => {
      disposed = true;
      cancelled = true;
      cancelAnimationFrame(frame);
      resizeObserver.disconnect();
      controls.dispose();
      renderer.dispose();
      renderer.forceContextLoss();
      container.removeChild(renderer.domElement);
      jumpToRoomRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hall]);

  const total = hall?.placed.length ?? 0;

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0, minHeight: 0, position: "relative" }}>
      <div style={{ padding: "14px 20px", borderBottom: "1px solid var(--border)", display: "flex", alignItems: "center", gap: 14 }}>
        <div>
          <div style={{ font: "700 15px system-ui" }}>{lang === "uk" ? "🏛 Вітрина" : "🏛 Showroom"}</div>
          <div style={{ font: "500 11px system-ui", color: "var(--text-faint)" }}>
            {lang === "uk"
              ? "Синтетична зала з реальних предметів і персонажів гри — не справжня локація, а вітрина, зібрана застосунком."
              : "A synthetic hall of the game's real items and characters — not a real location, a showroom the app assembled itself."}
          </div>
        </div>
        <div style={{ flex: 1 }} />
        {hall?.roomStarts.map((r) => (
          <button
            key={r.id}
            onClick={() => jumpToRoomRef.current?.(r.z)}
            style={{ padding: "7px 14px", borderRadius: 9, background: "var(--bg2)", border: "1px solid var(--border)", font: "600 11.5px system-ui", color: "var(--text)", whiteSpace: "nowrap" }}
          >
            {lang === "uk" ? r.label.uk : r.label.en}
          </button>
        ))}
      </div>
      <div ref={containerRef} style={{ flex: 1, minHeight: 0, position: "relative" }} />
      {error ? (
        <div style={{ position: "absolute", bottom: 14, left: 20, color: "var(--red)", font: "500 12px system-ui" }}>{error}</div>
      ) : hall && loaded < total ? (
        <div style={{ position: "absolute", bottom: 14, left: 20, font: "600 11px system-ui", color: "var(--text-faint)", background: "rgba(0,0,0,.5)", padding: "6px 12px", borderRadius: 8 }}>
          {lang === "uk" ? `Завантаження… ${loaded}/${total}` : `Loading… ${loaded}/${total}`}
        </div>
      ) : !hall ? (
        <div style={{ position: "absolute", inset: 0, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-faint)" }}>
          {lang === "uk" ? "Готую вітрину…" : "Preparing the showroom…"}
        </div>
      ) : null}
    </div>
  );
}
