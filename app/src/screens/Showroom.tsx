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
import { gridPositions, gridRowCount, normalizeScale, stackZones, type Vec3 } from "../lib/showroomLayout";

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
  /** Game weapon meshes are authored lying flat (as dropped on the ground / an inventory icon) —
   * their longest local axis is X or Z, not Y. Wall-mounted display needs them standing upright
   * instead, so `"vertical"` tells `loadOne` to rotate the mesh's longest axis onto world Y before
   * placing it. Omitted (undefined) leaves the mesh in its native orientation, which is correct
   * for things meant to lie flat on a table (food/valuables/tools) or that are already upright by
   * nature (actors). */
  orient?: "vertical";
  /** Recentering an object on its bounding-box CENTER (the default) puts that center at the
   * placed Y — fine for a wall-mounted display anchor, but for anything meant to stand/rest ON a
   * surface (a character on the floor, an item on a table) it buries the bottom half below that
   * surface (owner screenshots, 2026-07-20: "Персонажі на половину в землі" / "Припаси і скарби
   * також на половину в землі"). `true` recenters X/Z on the bbox center as usual but aligns Y so
   * the bbox's BOTTOM sits at the placed Y instead. */
  grounded?: boolean;
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
    placed.push({ key: `mesh:${swords[i].entryPath}`, kind: "mesh", entry: swords[i], position: p, targetSize: 1.3, orient: "vertical" }),
  );
  const shields = itemsByZone.shields;
  const shieldCols = 4;
  gridPositions({ count: shields.length, columns: shieldCols, cellSize: 1.8, origin: [11, 5, room1Z + 3], axis: "wall" }).forEach((p, i) =>
    placed.push({ key: `mesh:${shields[i].entryPath}`, kind: "mesh", entry: shields[i], position: p, targetSize: 1.6, orient: "vertical" }),
  );
  const weaponsMisc = itemsByZone.weaponsMisc;
  const miscCols = 8;
  const miscRows = gridRowCount(weaponsMisc.length, miscCols);
  gridPositions({ count: weaponsMisc.length, columns: miscCols, cellSize: 1.6, origin: [-((miscCols - 1) * 1.6) / 2, 1.1, room1Z + 4], axis: "floor" }).forEach((p, i) =>
    placed.push({ key: `mesh:${weaponsMisc[i].entryPath}`, kind: "mesh", entry: weaponsMisc[i], position: p, targetSize: 1.1, grounded: true }),
  );
  const room1Depth = Math.max(16, miscRows * 1.6 + 8);

  // --- Room 2: the hall of figures — humans, then monsters, then mobs, each its own rank
  // standing shoulder-to-shoulder facing down the hall (a lineup against the back wall), not a
  // single file receding into the distance — wraps to a new row behind itself once a rank gets
  // too wide for one line. ---
  const room2Z = room1Z + room1Depth + ROOM_GAP;
  roomStarts.push({ id: "figures", label: { uk: "🧍 Персонажі", en: "🧍 Figures" }, z: room2Z });
  const humans = actorsByZone.humans;
  const monsters = actorsByZone.monsters;
  const mobs = actorsByZone.mobs;
  const figureCols = 10;
  const humansRows = gridRowCount(humans.length, figureCols);
  const monstersRows = gridRowCount(monsters.length, figureCols);
  const mobsRows = gridRowCount(mobs.length, figureCols);
  const figureRankZ = stackZones(
    [
      { id: "humans", depth: humansRows * FIGURE_SPACING },
      { id: "monsters", depth: monstersRows * FIGURE_SPACING },
      { id: "mobs", depth: mobsRows * FIGURE_SPACING },
    ],
    room2Z + 3,
    3,
  );
  const figureOriginX = -((figureCols - 1) * FIGURE_SPACING) / 2;
  gridPositions({ count: humans.length, columns: figureCols, cellSize: FIGURE_SPACING, origin: [figureOriginX, 0, figureRankZ.humans], axis: "floor" }).forEach((p, i) =>
    placed.push({ key: `actor:${humans[i].entryPath}`, kind: "actor", entry: humans[i], position: p, targetSize: 2, grounded: true }),
  );
  gridPositions({ count: monsters.length, columns: figureCols, cellSize: FIGURE_SPACING, origin: [figureOriginX, 0, figureRankZ.monsters], axis: "floor" }).forEach((p, i) =>
    placed.push({ key: `actor:${monsters[i].entryPath}`, kind: "actor", entry: monsters[i], position: p, targetSize: 2.2, grounded: true }),
  );
  gridPositions({ count: mobs.length, columns: figureCols, cellSize: FIGURE_SPACING, origin: [figureOriginX, 0, figureRankZ.mobs], axis: "floor" }).forEach((p, i) =>
    placed.push({ key: `actor:${mobs[i].entryPath}`, kind: "actor", entry: mobs[i], position: p, targetSize: 1.8, grounded: true }),
  );
  const room2Depth = figureRankZ.mobs - room2Z + mobsRows * FIGURE_SPACING + 3;

  // --- Room 3: provisions & curios — four tables side by side: food, valuables, potions,
  // tools/books. Potions got their own table (2026-07-21, owner request) instead of being
  // dumped into the generic tools bucket — real Item_Flask_* items (8 total: 4 numbered
  // potions + Health/Mana/Misc/Empty). ---
  const room3Z = room2Z + room2Depth + ROOM_GAP;
  roomStarts.push({ id: "provisions", label: { uk: "🍞 Припаси й скарби", en: "🍞 Provisions & curios" }, z: room3Z });
  const food = itemsByZone.food;
  const valuables = itemsByZone.valuables;
  const potions = itemsByZone.potions;
  const tools = itemsByZone.tools;
  const foodCols = 10;
  const valuableCols = 8;
  const potionCols = 4;
  const toolCols = 10;
  const room3Depth =
    Math.max(
      gridRowCount(food.length, foodCols),
      gridRowCount(valuables.length, valuableCols),
      gridRowCount(potions.length, potionCols),
      gridRowCount(tools.length, toolCols),
    ) *
      1.1 +
    6;
  gridPositions({ count: food.length, columns: foodCols, cellSize: 1.0, origin: [-16, 1, room3Z + 3], axis: "floor" }).forEach((p, i) =>
    placed.push({ key: `mesh:${food[i].entryPath}`, kind: "mesh", entry: food[i], position: p, targetSize: 0.6, grounded: true }),
  );
  gridPositions({ count: valuables.length, columns: valuableCols, cellSize: 0.9, origin: [-3, 1, room3Z + 3], axis: "floor" }).forEach((p, i) =>
    placed.push({ key: `mesh:${valuables[i].entryPath}`, kind: "mesh", entry: valuables[i], position: p, targetSize: 0.4, grounded: true }),
  );
  gridPositions({ count: potions.length, columns: potionCols, cellSize: 0.8, origin: [8, 1, room3Z + 3], axis: "floor" }).forEach((p, i) =>
    placed.push({ key: `mesh:${potions[i].entryPath}`, kind: "mesh", entry: potions[i], position: p, targetSize: 0.5, grounded: true }),
  );
  gridPositions({ count: tools.length, columns: toolCols, cellSize: 1.0, origin: [18, 1, room3Z + 3], axis: "floor" }).forEach((p, i) =>
    placed.push({ key: `mesh:${tools[i].entryPath}`, kind: "mesh", entry: tools[i], position: p, targetSize: 0.7, grounded: true }),
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
  // Owner request (2026-07-21): see the hall with either the untouched original textures or
  // whatever AI-remastered variant exists so far. Not gated on review-approval status — "edited"
  // here means the same thing it does everywhere else in this app (AiCompare, Library thumbnails):
  // a variant that exists on disk, reviewed or not. Switching modes fully rebuilds the scene
  // (see the `mode` dependency on the big scene effect below) rather than trying to hot-swap
  // textures on already-built materials — simpler, and this is an occasional toggle, not a
  // per-frame concern.
  const [mode, setMode] = useState<"original" | "remastered">("original");
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
    const itemsByZone: Record<ItemZoneId, MeshEntry[]> = { swords: [], shields: [], weaponsMisc: [], food: [], valuables: [], potions: [], tools: [] };
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
      const { readTextureDataUrl, readEditedDataUrl } = await import("../lib/api");
      if (mode === "remastered") {
        try {
          return await readEditedDataUrl(entry.pngRel);
        } catch {
          // No AI-approved variant for this particular texture yet — fall back to the
          // original rather than leaving the material untextured.
        }
      }
      return readTextureDataUrl(entry.pngRel);
    };
  }, [textures, mode]);

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

    // WASD walking, constant speed regardless of orbit distance — mouse-wheel zoom through
    // OrbitControls is multiplicative (each tick scales the remaining distance), so covering the
    // hall's real length that way gets slower the farther out you are (owner report, 2026-07-20:
    // "чим дальше рухаєшся тим помаліше наближаєшся"). Moving camera + orbit target together by
    // the same fixed-speed vector each frame sidesteps that entirely — same ground speed at any
    // distance, and OrbitControls' own update() keeps the orbit offset intact since both move
    // together (the standard "fly the whole rig" technique, not fighting the library).
    const pressedKeys = new Set<string>();
    const WALK_KEYS = new Set(["w", "a", "s", "d"]);
    function onKeyDown(e: KeyboardEvent) {
      const k = e.key.toLowerCase();
      if (WALK_KEYS.has(k)) pressedKeys.add(k);
    }
    function onKeyUp(e: KeyboardEvent) {
      pressedKeys.delete(e.key.toLowerCase());
    }
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    const WALK_SPEED = 14; // world units / second
    const forward = new THREE.Vector3();
    const right = new THREE.Vector3();
    const moveDelta = new THREE.Vector3();
    function applyWalk(deltaSeconds: number) {
      if (pressedKeys.size === 0) return;
      camera.getWorldDirection(forward);
      forward.y = 0;
      if (forward.lengthSq() < 1e-6) return;
      forward.normalize();
      right.crossVectors(forward, camera.up).normalize();
      moveDelta.set(0, 0, 0);
      if (pressedKeys.has("w")) moveDelta.add(forward);
      if (pressedKeys.has("s")) moveDelta.sub(forward);
      if (pressedKeys.has("d")) moveDelta.add(right);
      if (pressedKeys.has("a")) moveDelta.sub(right);
      if (moveDelta.lengthSq() < 1e-6) return;
      moveDelta.normalize().multiplyScalar(WALK_SPEED * deltaSeconds);
      camera.position.add(moveDelta);
      controls.target.add(moveDelta);
    }

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
    // The shield grid (4 cols × 1.8 cellSize, from x=11) spans out to x=16.4 — a wall at the
    // OLD x=13 sat mid-grid, clipping straight through the second column of shields (owner
    // screenshot, 2026-07-20: "Щити в стіні"). 18 clears the grid's full extent with margin.
    const rightWall = leftWall.clone();
    rightWall.position.set(18, 6, 6);
    scene.add(rightWall);

    const tableMat = new THREE.MeshStandardMaterial({ color: 0x3a2c20, roughness: 0.8 });
    const table1 = new THREE.Mesh(new THREE.BoxGeometry(15, 1, 6), tableMat);
    table1.position.set(0, 0.55, 8);
    scene.add(table1);
    const room3Start = hall.roomStarts.find((r) => r.id === "provisions")?.z ?? hall.totalDepth - 20;
    // food / valuables / potions / tools — potions (2026-07-21) gets a narrower table since it's
    // only ever 8 real items (Item_Flask_Potion_01-04/Health/Mana/Misc/Empty), not a full row.
    for (const [x, width] of [
      [-16, 11],
      [-3, 11],
      [8, 5],
      [18, 11],
    ] as [number, number][]) {
      const t = new THREE.Mesh(new THREE.BoxGeometry(width, 1, 8), tableMat.clone());
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
      if (item.orient === "vertical") {
        // Weapon meshes are authored lying flat (as dropped on the ground / an inventory icon):
        // their longest dimension sits on local X or Z, not Y. Rotate whichever local axis is
        // longest onto world Y BEFORE the final bbox/recenter below, so a wall-mounted sword
        // reads as a real hanging blade instead of floating at whatever diagonal its native mesh
        // orientation happened to have (owner report, 2026-07-20 screenshot: swords scattered at
        // random angles off the weapon wall).
        const rawSize = new THREE.Box3().setFromObject(object).getSize(new THREE.Vector3());
        if (rawSize.x >= rawSize.y && rawSize.x >= rawSize.z) object.rotation.z = Math.PI / 2;
        else if (rawSize.z >= rawSize.y && rawSize.z >= rawSize.x) object.rotation.x = -Math.PI / 2;
      }
      const box = new THREE.Box3().setFromObject(object); // AFTER rotation, so this bbox reflects final orientation
      const size = box.getSize(new THREE.Vector3());
      const center = box.getCenter(new THREE.Vector3());
      // X/Z always recenter on the bbox center (so a grid column/row lines up with the object's
      // horizontal middle). Y defaults to the same center — right for a wall-mounted display
      // anchor — but anything meant to REST on a surface needs its bbox BOTTOM at the placed Y
      // instead, or half of it renders below the floor/table (owner screenshots, 2026-07-20:
      // "Персонажі на половину в землі", "Припаси і скарби також на половину в землі").
      object.position.sub(new THREE.Vector3(center.x, item.grounded ? box.min.y : center.y, center.z));
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
    let lastFrameTime = performance.now();
    function animate() {
      frame = requestAnimationFrame(animate);
      const now = performance.now();
      const deltaSeconds = Math.min(0.1, (now - lastFrameTime) / 1000); // cap avoids a huge jump after a tab was backgrounded
      lastFrameTime = now;
      applyWalk(deltaSeconds);
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
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      resizeObserver.disconnect();
      controls.dispose();
      renderer.dispose();
      renderer.forceContextLoss();
      container.removeChild(renderer.domElement);
      jumpToRoomRef.current = null;
    };
    // `mode` is a real dependency, not an oversight: switching original/remastered needs
    // `resolveTextureRef.current` to already reflect the new mode (the other effect above
    // updates it synchronously first, same render) AND every material rebuilt from scratch,
    // since `applyMaterials`' per-name texture-load Promise only ever runs once per material.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hall, mode]);

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
        <div style={{ display: "flex", gap: 6 }}>
          {(
            [
              ["original", lang === "uk" ? "Оригінал" : "Original"],
              ["remastered", lang === "uk" ? "Ремастед" : "Remastered"],
            ] as [typeof mode, string][]
          ).map(([id, label]) => {
            const active = mode === id;
            return (
              <button
                key={id}
                onClick={() => setMode(id)}
                style={{
                  padding: "7px 14px",
                  borderRadius: 9,
                  background: active ? "var(--accent)" : "var(--bg2)",
                  border: `1px solid ${active ? "var(--accent)" : "var(--border)"}`,
                  font: "600 11.5px system-ui",
                  color: active ? "#fff" : "var(--text)",
                  whiteSpace: "nowrap",
                }}
              >
                {label}
              </button>
            );
          })}
        </div>
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
